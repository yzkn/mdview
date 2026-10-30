//! 埋め込み描画のワーカープール（§4.3）。
//!
//! **UI スレッドを止めない**ことがすべてである。依頼は投げっぱなしにし、
//! 結果は毎フレーム [`EmbedPool::poll`] で拾う。
//!
//! tokio を使わない。仕事は CPU を使う描画であり、非同期ランタイムの
//! 利点が無い。標準のスレッドとチャネルで足りる。

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{EmbedError, EmbedKey, EmbedSource, RenderEmbed, RenderedEmbed};

/// 1 件の状態。
#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    /// 依頼済み。まだ届いていない
    Pending,
    Done(RenderedEmbed),
    Failed(EmbedError),
}

type Completion = (EmbedKey, Result<RenderedEmbed, EmbedError>);

/// 本文が変わってから描き直すまでの間（§16.12）。
///
/// **打鍵のたびに描くと、構文の途中の図が点滅する。**
pub const DEBOUNCE: Duration = Duration::from_millis(300);

/// 持っておく件数の上限（§16.8）。
pub const MAX_ENTRIES: usize = 200;

/// 持っておく画素の合計（同上）。
pub const MAX_BYTES: usize = 128 * 1024 * 1024;

/// 1 件分の持ち物。
struct Entry {
    state: JobState,
    /// 最後に欲しがられた順番。**小さいほど古い**
    used: u64,
    /// 画素の大きさ。合計の上限を見るのに使う
    bytes: usize,
}

/// 描画の依頼を捌く。
pub struct EmbedPool {
    /// 仕事の投入口。**落とすとワーカーが終わる**（`Drop` で使う）
    jobs: Option<Sender<(EmbedKey, EmbedSource)>>,
    done_rx: Receiver<Completion>,
    /// 鍵ごとの状態。UI スレッドだけが触る
    entries: HashMap<EmbedKey, Entry>,
    workers: Vec<JoinHandle<()>>,
    /// まだ投げていない依頼。**編集中だけ溜まる**
    queued: HashMap<EmbedKey, EmbedSource>,
    /// いま画面が欲しがっている鍵。追い出しから守る
    wanted: HashSet<EmbedKey>,
    /// 最後に本文が変わった時刻
    touched: Option<Instant>,
    /// 使われた順番を数える
    clock: u64,
    /// 画素の合計
    bytes: usize,
    /// 件数の上限。**試験だけが下げる**
    max_entries: usize,
    /// 画素の合計の上限。同上
    max_bytes: usize,
}

impl EmbedPool {
    /// 既定のワーカー数。
    ///
    /// **CPU 数より控えめにする。** UI スレッドが 1 本要るうえ、
    /// 図の描画はメモリ帯域も食う。
    fn default_workers() -> usize {
        std::thread::available_parallelism()
            .map(|n| (n.get().saturating_sub(1)).clamp(1, 4))
            .unwrap_or(2)
    }

    pub fn new(renderer: Arc<dyn RenderEmbed>) -> Self {
        Self::with_workers(renderer, Self::default_workers())
    }

    pub fn with_workers(renderer: Arc<dyn RenderEmbed>, workers: usize) -> Self {
        let (jobs_tx, jobs_rx) = channel::<(EmbedKey, EmbedSource)>();
        let (done_tx, done_rx) = channel::<Completion>();
        // 受け口を全ワーカーで分け合う。先に取った者が処理する
        let jobs_rx = Arc::new(Mutex::new(jobs_rx));

        let workers = (0..workers.max(1))
            .map(|_| {
                let jobs_rx = Arc::clone(&jobs_rx);
                let done_tx = done_tx.clone();
                let renderer = Arc::clone(&renderer);
                std::thread::spawn(move || loop {
                    // **ロックを持ったまま描かない。** 取り出したらすぐ手放す
                    let job = {
                        let guard = jobs_rx.lock().expect("ワーカーの受け口が壊れた");
                        guard.recv()
                    };
                    let Ok((key, source)) = job else {
                        // 投入口が落ちた = 終了
                        return;
                    };
                    // **1 件落ちても糸を殺さない**（§10.32）。
                    // 殺すとプールが痩せ、以後の図が描かれなくなる
                    let result = crate::worker::catch("図の描画", || renderer.render(&source))
                        .unwrap_or_else(|reason| Err(EmbedError::Failed(reason)));
                    if done_tx.send((key, result)).is_err() {
                        // 受け取り手が消えた = アプリ終了中
                        return;
                    }
                })
            })
            .collect();

        Self {
            jobs: Some(jobs_tx),
            done_rx,
            entries: HashMap::new(),
            workers,
            queued: HashMap::new(),
            wanted: HashSet::new(),
            touched: None,
            clock: 0,
            bytes: 0,
            max_entries: MAX_ENTRIES,
            max_bytes: MAX_BYTES,
        }
    }

    /// 上限を下げる。**試験専用。**
    ///
    /// 128MB を本当に積むと試験が端末を圧迫する。追い出しの筋道は
    /// 件数と画素で共通なので、小さくしても同じところを通る。
    #[cfg(test)]
    fn set_limits(&mut self, entries: usize, bytes: usize) {
        self.max_entries = entries;
        self.max_bytes = bytes;
    }

    /// 本文が変わったことを伝える（§16.12）。
    ///
    /// **編集の入口から 1 度だけ呼ぶ。** ここから [`DEBOUNCE`] の間は
    /// 新しい依頼を投げずに溜める。
    pub fn touch(&mut self) {
        self.touch_at(Instant::now());
    }

    /// 時刻を渡す版。**試験はこちらを使う**（眠らせない）。
    pub fn touch_at(&mut self, now: Instant) {
        self.touched = Some(now);
    }

    /// 編集が落ち着くのを待っている最中か。
    fn settling(&self, now: Instant) -> bool {
        matches!(self.touched, Some(at) if now.duration_since(at) < DEBOUNCE)
    }

    /// 画面が欲しがっているものを**まとめて**伝える。
    ///
    /// **1 件ずつ渡してはいけない。** まとめて渡さないと「もう要らなく
    /// なったもの」が分からず、打鍵の途中の図を描いてしまう。
    pub fn sync(&mut self, wanted: impl IntoIterator<Item = EmbedSource>) {
        self.sync_at(wanted, Instant::now());
    }

    /// 時刻を渡す版。
    pub fn sync_at(&mut self, wanted: impl IntoIterator<Item = EmbedSource>, now: Instant) {
        self.wanted.clear();
        for source in wanted {
            self.request_at(source, now);
        }
        self.flush_queue(now);
        self.evict();
    }

    /// 描画を頼む。**同じ鍵を二重に投げない。**
    ///
    /// 可視範囲は毎フレーム同じものを並べるので、これが無いと
    /// 1 秒間に 60 回同じ図を描き直すことになる。
    pub fn request(&mut self, source: EmbedSource) -> &JobState {
        self.request_at(source, Instant::now())
    }

    /// 時刻を渡す版。
    pub fn request_at(&mut self, source: EmbedSource, now: Instant) -> &JobState {
        let key = source.key();
        self.wanted.insert(key);

        if self.entries.contains_key(&key) {
            // **引いたら新しくする。** これが LRU の「使った」印である
            self.clock += 1;
            let clock = self.clock;
            if let Some(entry) = self.entries.get_mut(&key) {
                entry.used = clock;
            }
            return &self.entries[&key].state;
        }

        if self.settling(now) {
            // **編集が落ち着くまで投げない**（§16.12）。
            // 箱の大きさは決まるよう、状態だけは未確定として置く
            self.remember(key, JobState::Pending);
            self.queued.insert(key, source);
        } else {
            self.dispatch(key, source);
        }
        &self.entries[&key].state
    }

    /// 実際にワーカーへ投げる。
    fn dispatch(&mut self, key: EmbedKey, source: EmbedSource) {
        self.remember(key, JobState::Pending);
        if let Some(jobs) = &self.jobs {
            // 送れない＝ワーカーが全滅。**握りつぶさず失敗として残す**
            if jobs.send((key, source)).is_err() {
                self.remember(
                    key,
                    JobState::Failed(EmbedError::Failed("ワーカーが停止している".to_owned())),
                );
            }
        }
    }

    /// 溜めてあった依頼を投げる。
    fn flush_queue(&mut self, now: Instant) {
        // **要らなくなったものは投げずに捨てる。**
        // 打鍵の途中の内容がこれに当たる（DD-OPEN-15）
        let stale: Vec<EmbedKey> = self
            .queued
            .keys()
            .filter(|key| !self.wanted.contains(*key))
            .copied()
            .collect();
        for key in stale {
            self.queued.remove(&key);
            // まだ投げていないので、未確定の印も消す
            if matches!(
                self.entries.get(&key).map(|e| &e.state),
                Some(JobState::Pending)
            ) {
                self.forget(&key);
            }
        }

        if self.settling(now) {
            return;
        }
        for (key, source) in std::mem::take(&mut self.queued) {
            self.dispatch(key, source);
        }
    }

    /// 状態を覚える。画素の合計もここで直す。
    fn remember(&mut self, key: EmbedKey, state: JobState) {
        let bytes = match &state {
            JobState::Done(embed) => embed.pixels.len(),
            _ => 0,
        };
        self.clock += 1;
        let entry = Entry {
            state,
            used: self.clock,
            bytes,
        };
        if let Some(old) = self.entries.insert(key, entry) {
            self.bytes -= old.bytes;
        }
        self.bytes += bytes;
    }

    /// 1 件捨てる。
    fn forget(&mut self, key: &EmbedKey) {
        if let Some(old) = self.entries.remove(key) {
            self.bytes -= old.bytes;
        }
    }

    /// 上限を超えた分を古いものから捨てる（§16.8）。
    ///
    /// **捨てられるのは「届いていて、いま要らないもの」だけである。**
    /// 未確定を捨てると結果が戻ってきたときに持ち主がおらず、
    /// 画面に出ているものを捨てると次のフレームで描き直しになる。
    fn evict(&mut self) {
        if self.entries.len() <= self.max_entries && self.bytes <= self.max_bytes {
            return;
        }
        let mut removable: Vec<(u64, EmbedKey)> = self
            .entries
            .iter()
            .filter(|(key, entry)| {
                !matches!(entry.state, JobState::Pending) && !self.wanted.contains(*key)
            })
            .map(|(key, entry)| (entry.used, *key))
            .collect();
        removable.sort_unstable_by_key(|(used, _)| *used);

        for (_, key) in removable {
            if self.entries.len() <= self.max_entries && self.bytes <= self.max_bytes {
                break;
            }
            self.forget(&key);
        }
    }

    /// 届いた結果を取り込む。**待たない。**
    ///
    /// 戻り値は今回新しく確定した鍵。呼び出し側は高さ索引を直すのに使う。
    pub fn poll(&mut self) -> Vec<EmbedKey> {
        self.poll_at(Instant::now())
    }

    /// 時刻を渡す版。
    pub fn poll_at(&mut self, now: Instant) -> Vec<EmbedKey> {
        let mut settled = Vec::new();
        while let Ok((key, result)) = self.done_rx.try_recv() {
            let state = match result {
                Ok(embed) => JobState::Done(embed),
                Err(error) => JobState::Failed(error),
            };
            self.remember(key, state);
            settled.push(key);
        }
        // **ここでも溜まった分を投げる。** 画面の更新が止まっていても
        // 点滅用の刻み（500ms）でここへ来る
        self.flush_queue(now);
        self.evict();
        settled
    }

    /// 状態を引く。依頼していなければ `None`。
    ///
    /// **ここでは LRU の順番を直さない。** 引き口は描画のたびに
    /// 不変参照で呼ばれる（[`crate::layout::EmbedLookup`]）。
    /// 順番は [`Self::request_at`] で付ける。
    pub fn get(&self, key: &EmbedKey) -> Option<&JobState> {
        self.entries.get(key).map(|entry| &entry.state)
    }

    /// 依頼済みの件数（試験と状態表示用）。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 持っている画素の合計（試験用）。
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// まだ投げていない件数（試験用）。
    pub fn queued(&self) -> usize {
        self.queued.len()
    }

    /// まだ届いていない件数。
    pub fn pending(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| matches!(entry.state, JobState::Pending))
            .count()
    }
}

impl Drop for EmbedPool {
    fn drop(&mut self) {
        // **投入口を先に落とす。** これでワーカーの `recv` が終わる
        self.jobs = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

impl std::fmt::Debug for EmbedPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbedPool")
            .field("workers", &self.workers.len())
            .field("jobs", &self.entries.len())
            .field("pending", &self.pending())
            .field("queued", &self.queued.len())
            .field("bytes", &self.bytes)
            .finish()
    }
}

#[cfg(test)]
mod debounce_and_cap_tests {
    use super::*;
    use crate::embed::EmbedKind;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    /// 何を何回描いたかを数えるだけの描画器。
    struct Tally {
        calls: AtomicUsize,
        /// 1 件あたりの画素の大きさ（バイト）
        bytes: usize,
    }

    impl Tally {
        fn new(bytes: usize) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                bytes,
            })
        }
    }

    impl RenderEmbed for Tally {
        fn render(&self, _source: &EmbedSource) -> Result<RenderedEmbed, EmbedError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(RenderedEmbed {
                width: 1,
                height: 1,
                pixels: Arc::new(vec![0; self.bytes]),
            })
        }
    }

    fn source(text: &str) -> EmbedSource {
        EmbedSource::new(EmbedKind::Diagram, text, 800.0)
    }

    /// 全部届くまで回す。**時刻は渡さない**（溜め込みに影響させない）。
    fn drain(pool: &mut EmbedPool, count: usize, now: Instant) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut settled = 0;
        while settled < count && Instant::now() < deadline {
            settled += pool.poll_at(now).len();
            std::thread::yield_now();
        }
        assert_eq!(settled, count, "結果が届かない");
    }

    /// **編集中は投げない**（§16.12）。
    ///
    /// 打鍵のたびに描くと、構文の途中の図が点滅する
    #[test]
    fn an_edit_holds_the_request_back() {
        let renderer = Tally::new(4);
        let mut pool = EmbedPool::with_workers(Arc::clone(&renderer) as Arc<dyn RenderEmbed>, 1);

        let t0 = Instant::now();
        pool.touch_at(t0);
        pool.sync_at([source("図")], t0 + Duration::from_millis(10));

        assert_eq!(pool.queued(), 1, "溜めていない");
        assert_eq!(renderer.calls.load(Ordering::SeqCst), 0, "もう描いている");
        // 箱の大きさが決まるよう、状態は未確定として見える
        assert_eq!(
            *pool.get(&source("図").key()).expect("状態がある"),
            JobState::Pending
        );
    }

    /// **止まってから投げる。**
    #[test]
    fn it_goes_out_once_the_typing_settles() {
        let renderer = Tally::new(4);
        let mut pool = EmbedPool::with_workers(Arc::clone(&renderer) as Arc<dyn RenderEmbed>, 1);

        let t0 = Instant::now();
        pool.touch_at(t0);
        pool.sync_at([source("図")], t0 + Duration::from_millis(10));
        assert_eq!(pool.queued(), 1);

        // 300ms 経ったところで同じものを欲しがる
        let after = t0 + DEBOUNCE + Duration::from_millis(1);
        pool.sync_at([source("図")], after);
        assert_eq!(pool.queued(), 0, "まだ溜めている");

        drain(&mut pool, 1, after);
        assert_eq!(renderer.calls.load(Ordering::SeqCst), 1);
    }

    /// **要らなくなったものは投げずに捨てる。**
    ///
    /// 打鍵の途中の内容がこれに当たる（DD-OPEN-15）
    #[test]
    fn an_abandoned_request_is_never_rendered() {
        let renderer = Tally::new(4);
        let mut pool = EmbedPool::with_workers(Arc::clone(&renderer) as Arc<dyn RenderEmbed>, 1);

        let t0 = Instant::now();
        // 打鍵のたびに中身が変わる
        for (step, text) in ["g", "gr", "gra", "grap", "graph"].iter().enumerate() {
            let now = t0 + Duration::from_millis(step as u64 * 50);
            pool.touch_at(now);
            pool.sync_at([source(text)], now);
        }
        assert_eq!(pool.queued(), 1, "最後の 1 件だけが残る");
        assert_eq!(pool.len(), 1, "途中の内容が残っている");

        // 止まった
        let after = t0 + Duration::from_millis(200) + DEBOUNCE + Duration::from_millis(1);
        pool.sync_at([source("graph")], after);
        drain(&mut pool, 1, after);

        // **描いたのは最後の 1 件だけ**
        assert_eq!(
            renderer.calls.load(Ordering::SeqCst),
            1,
            "途中の図まで描いている"
        );
    }

    /// 編集していなければ待たせない（開いた直後に 300ms 待たせない）。
    #[test]
    fn without_an_edit_it_goes_out_at_once() {
        let renderer = Tally::new(4);
        let mut pool = EmbedPool::with_workers(Arc::clone(&renderer) as Arc<dyn RenderEmbed>, 1);

        let t0 = Instant::now();
        pool.sync_at([source("図")], t0);
        assert_eq!(pool.queued(), 0, "待たせている");
        drain(&mut pool, 1, t0);
        assert_eq!(renderer.calls.load(Ordering::SeqCst), 1);
    }

    /// **件数の上限を超えたら古いものから捨てる**（§16.8）。
    #[test]
    fn the_oldest_entries_are_dropped_past_the_count_limit() {
        let mut pool = EmbedPool::with_workers(Tally::new(4) as Arc<dyn RenderEmbed>, 1);
        pool.set_limits(3, usize::MAX);

        let t0 = Instant::now();
        for index in 0..6 {
            let text = format!("図 {index}");
            pool.sync_at([source(&text)], t0);
            drain(&mut pool, 1, t0);
        }
        // 最後に欲しがった 1 件は残り、全体は上限内
        assert!(pool.len() <= 3, "上限を超えて持っている: {}", pool.len());
        assert!(
            pool.get(&source("図 5").key()).is_some(),
            "最新を捨てている"
        );
        assert!(
            pool.get(&source("図 0").key()).is_none(),
            "最古が残っている"
        );
    }

    /// **画素の合計でも捨てる**（同上）。
    #[test]
    fn the_byte_total_also_forces_eviction() {
        // 1 件 1,000 バイト。合計 2,500 バイトまで
        let mut pool = EmbedPool::with_workers(Tally::new(1_000) as Arc<dyn RenderEmbed>, 1);
        pool.set_limits(usize::MAX, 2_500);

        let t0 = Instant::now();
        for index in 0..6 {
            let text = format!("図 {index}");
            pool.sync_at([source(&text)], t0);
            drain(&mut pool, 1, t0);
        }
        assert!(
            pool.bytes() <= 2_500,
            "画素が溜まっている: {}",
            pool.bytes()
        );
        assert!(pool.len() <= 3, "{} 件ある", pool.len());
    }

    /// **いま画面に出ているものは捨てない。**
    ///
    /// 捨てると次のフレームで描き直しになり、点滅する
    #[test]
    fn what_the_screen_wants_is_kept() {
        let mut pool = EmbedPool::with_workers(Tally::new(4) as Arc<dyn RenderEmbed>, 1);
        pool.set_limits(1, usize::MAX);

        let t0 = Instant::now();
        // 3 件を同時に欲しがる。上限は 1 件だが、どれも画面に出ている
        let wanted = ["図 A", "図 B", "図 C"];
        pool.sync_at(wanted.iter().map(|text| source(text)), t0);
        drain(&mut pool, 3, t0);
        pool.sync_at(wanted.iter().map(|text| source(text)), t0);

        for text in wanted {
            assert!(pool.get(&source(text).key()).is_some(), "{text} を捨てた");
        }
    }

    /// **未確定は捨てない。** 結果が戻ってきたときに持ち主がいなくなる
    #[test]
    fn pending_entries_survive_eviction() {
        let mut pool = EmbedPool::with_workers(Tally::new(4) as Arc<dyn RenderEmbed>, 1);
        pool.set_limits(0, 0);

        let t0 = Instant::now();
        pool.sync_at([source("図")], t0);
        // 結果を取り込む前に、別のものを欲しがって追い出しを起こす
        pool.sync_at([source("別の図")], t0);
        assert!(!pool.is_empty(), "未確定まで捨てた");

        drain(&mut pool, 2, t0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::{EmbedKind, NotImplemented};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    /// **描画が落ちても理由が届き、糸は死なない**（§10.32）。
    ///
    /// 落ちた糸をそのままにすると、以後その分だけプールが痩せ、
    /// 図が「いつまでも描かれない」状態になる
    #[test]
    fn a_panicking_render_reports_and_keeps_the_worker() {
        // **糸を 1 本にする。** 死んだら次が描けないことを確実に見る
        let mut pool = EmbedPool::with_workers(Arc::new(Exploding), 1);

        pool.request(source("落ちる図"));
        let settled = wait_for(&mut pool, 1);
        assert_eq!(settled.len(), 1, "結果が返ってこない");

        let failed = pool.get(&settled[0]).expect("状態がある");
        match failed {
            JobState::Failed(EmbedError::Failed(reason)) => {
                assert!(reason.contains("図の中身が壊れている"), "{reason}");
            }
            other => panic!("落下が伝わっていない: {other:?}"),
        }

        // **同じ糸で次が描ける**（死んでいない）
        pool.request(source("ふつうの図"));
        let settled = wait_for(&mut pool, 1);
        assert_eq!(settled.len(), 1, "糸が死んでいる");
        assert!(matches!(pool.get(&settled[0]), Some(JobState::Done(_))));
    }

    /// 数を数える描画器。**同じ依頼が何回届いたか**を見る。
    struct Counting {
        calls: AtomicUsize,
        delay: Duration,
    }

    impl Counting {
        fn new(delay_ms: u64) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                delay: Duration::from_millis(delay_ms),
            })
        }
    }

    impl RenderEmbed for Counting {
        fn render(&self, source: &EmbedSource) -> Result<RenderedEmbed, EmbedError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            if source.text == "壊れている" {
                return Err(EmbedError::Failed("構文誤り".to_owned()));
            }
            Ok(RenderedEmbed {
                width: 400,
                height: 200,
                pixels: Arc::new(vec![0; 400 * 200 * 4]),
            })
        }
    }

    /// 描画の途中で落ちる描画器。**糸が死なないこと**を見るために使う。
    struct Exploding;

    impl RenderEmbed for Exploding {
        fn render(&self, source: &EmbedSource) -> Result<RenderedEmbed, EmbedError> {
            assert!(source.text != "落ちる図", "図の中身が壊れている");
            Ok(RenderedEmbed {
                width: 1,
                height: 1,
                pixels: Arc::new(vec![0; 4]),
            })
        }
    }

    /// 結果が届くまで回す。**試験が固まらないよう上限を置く。**
    fn wait_for(pool: &mut EmbedPool, count: usize) -> Vec<EmbedKey> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut settled = Vec::new();
        while settled.len() < count && Instant::now() < deadline {
            settled.extend(pool.poll());
            std::thread::sleep(Duration::from_millis(2));
        }
        settled
    }

    fn source(text: &str) -> EmbedSource {
        EmbedSource::new(EmbedKind::Diagram, text, 800.0)
    }

    #[test]
    fn result_comes_back() {
        let renderer = Counting::new(0);
        let mut pool = EmbedPool::with_workers(renderer, 2);

        let key = source("graph TD; A-->B").key();
        assert_eq!(*pool.request(source("graph TD; A-->B")), JobState::Pending);

        let settled = wait_for(&mut pool, 1);
        assert_eq!(settled, vec![key]);
        assert!(matches!(pool.get(&key), Some(JobState::Done(_))));
        assert_eq!(pool.pending(), 0);
    }

    /// **同じ依頼を二重に投げない。** 可視範囲は毎フレーム同じものを並べる。
    #[test]
    fn the_same_request_is_not_repeated() {
        let renderer = Counting::new(0);
        let calls = Arc::clone(&renderer);
        let mut pool = EmbedPool::with_workers(renderer, 2);

        for _ in 0..60 {
            pool.request(source("同じ図"));
        }
        wait_for(&mut pool, 1);

        assert_eq!(calls.calls.load(Ordering::SeqCst), 1, "1 回だけ描く");
        assert_eq!(pool.len(), 1);
    }

    /// 内容が変われば別の依頼になる。
    #[test]
    fn different_content_is_a_new_request() {
        let renderer = Counting::new(0);
        let calls = Arc::clone(&renderer);
        let mut pool = EmbedPool::with_workers(renderer, 2);

        pool.request(source("図 A"));
        pool.request(source("図 B"));
        wait_for(&mut pool, 2);

        assert_eq!(calls.calls.load(Ordering::SeqCst), 2);
        assert_eq!(pool.len(), 2);
    }

    /// **失敗も結果である。** 握りつぶさず状態として残す。
    #[test]
    fn failure_is_kept() {
        let mut pool = EmbedPool::with_workers(Counting::new(0), 1);
        let key = source("壊れている").key();
        pool.request(source("壊れている"));
        wait_for(&mut pool, 1);

        match pool.get(&key) {
            Some(JobState::Failed(EmbedError::Failed(reason))) => assert_eq!(reason, "構文誤り"),
            other => panic!("失敗が残っていない: {other:?}"),
        }
    }

    #[test]
    fn unsupported_is_reported() {
        let mut pool = EmbedPool::with_workers(Arc::new(NotImplemented), 1);
        let key = source("graph TD").key();
        pool.request(source("graph TD"));
        wait_for(&mut pool, 1);

        assert!(matches!(
            pool.get(&key),
            Some(JobState::Failed(EmbedError::Unsupported(
                EmbedKind::Diagram
            )))
        ));
    }

    /// **`poll` は待たない。** 描画が終わっていなくてもすぐ戻る。
    #[test]
    fn poll_does_not_block() {
        let mut pool = EmbedPool::with_workers(Counting::new(200), 1);
        pool.request(source("遅い図"));

        let started = Instant::now();
        let settled = pool.poll();
        assert!(
            started.elapsed() < Duration::from_millis(50),
            "poll が待っている: {:?}",
            started.elapsed()
        );
        assert!(settled.is_empty());
        assert_eq!(pool.pending(), 1);
    }

    /// 複数のワーカーが同時に働く。
    #[test]
    fn workers_run_in_parallel() {
        let mut pool = EmbedPool::with_workers(Counting::new(120), 3);
        for index in 0..3 {
            pool.request(source(&format!("図 {index}")));
        }

        let started = Instant::now();
        let settled = wait_for(&mut pool, 3);
        assert_eq!(settled.len(), 3);
        // 直列なら 360ms 以上かかる。並行なら大きく下回る
        assert!(
            started.elapsed() < Duration::from_millis(330),
            "直列に見える: {:?}",
            started.elapsed()
        );
    }

    /// 仕事を残したまま落としても固まらない。
    #[test]
    fn dropping_with_pending_jobs_terminates() {
        let mut pool = EmbedPool::with_workers(Counting::new(30), 2);
        for index in 0..8 {
            pool.request(source(&format!("図 {index}")));
        }
        let started = Instant::now();
        drop(pool);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "終了に時間がかかりすぎ: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn unknown_key_is_none() {
        let pool = EmbedPool::with_workers(Counting::new(0), 1);
        assert!(pool.get(&source("頼んでいない").key()).is_none());
        assert!(pool.is_empty());
    }
}
