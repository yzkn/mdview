//! 埋め込み描画のワーカープール（§4.3）。
//!
//! **UI スレッドを止めない**ことがすべてである。依頼は投げっぱなしにし、
//! 結果は毎フレーム [`EmbedPool::poll`] で拾う。
//!
//! tokio を使わない。仕事は CPU を使う描画であり、非同期ランタイムの
//! 利点が無い。標準のスレッドとチャネルで足りる。

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

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

/// 描画の依頼を捌く。
pub struct EmbedPool {
    /// 仕事の投入口。**落とすとワーカーが終わる**（`Drop` で使う）
    jobs: Option<Sender<(EmbedKey, EmbedSource)>>,
    done_rx: Receiver<Completion>,
    /// 鍵ごとの状態。UI スレッドだけが触る
    states: HashMap<EmbedKey, JobState>,
    workers: Vec<JoinHandle<()>>,
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
            states: HashMap::new(),
            workers,
        }
    }

    /// 描画を頼む。**同じ鍵を二重に投げない。**
    ///
    /// 可視範囲は毎フレーム同じものを並べるので、これが無いと
    /// 1 秒間に 60 回同じ図を描き直すことになる。
    pub fn request(&mut self, source: EmbedSource) -> &JobState {
        let key = source.key();
        if !self.states.contains_key(&key) {
            self.states.insert(key, JobState::Pending);
            if let Some(jobs) = &self.jobs {
                // 送れない＝ワーカーが全滅。**握りつぶさず失敗として残す**
                if jobs.send((key, source)).is_err() {
                    self.states.insert(
                        key,
                        JobState::Failed(EmbedError::Failed("ワーカーが停止している".to_owned())),
                    );
                }
            }
        }
        &self.states[&key]
    }

    /// 届いた結果を取り込む。**待たない。**
    ///
    /// 戻り値は今回新しく確定した鍵。呼び出し側は高さ索引を直すのに使う。
    pub fn poll(&mut self) -> Vec<EmbedKey> {
        let mut settled = Vec::new();
        while let Ok((key, result)) = self.done_rx.try_recv() {
            let state = match result {
                Ok(embed) => JobState::Done(embed),
                Err(error) => JobState::Failed(error),
            };
            self.states.insert(key, state);
            settled.push(key);
        }
        settled
    }

    /// 状態を引く。依頼していなければ `None`。
    pub fn get(&self, key: &EmbedKey) -> Option<&JobState> {
        self.states.get(key)
    }

    /// 依頼済みの件数（試験と状態表示用）。
    pub fn len(&self) -> usize {
        self.states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// まだ届いていない件数。
    pub fn pending(&self) -> usize {
        self.states
            .values()
            .filter(|state| **state == JobState::Pending)
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
            .field("jobs", &self.states.len())
            .field("pending", &self.pending())
            .finish()
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
