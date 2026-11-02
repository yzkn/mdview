// 画面の側の小さな仕掛け（v3 R-02）。Rust へは dioxus.send で知らせる。
//
// - 作業の印 ☐ を押す・ブロックを長押しする・リンクを押す
// - 道具の帯（太字・見出しなど）。**押しても入力欄の焦点を外さない**。変換中（IME）は何もしない
// - キーボードの高さを CSS の --kb に入れる（下から出る板をキーボードの上に置く）
// - 検索の印（CSS Custom Highlight。使えない WebView では印を付けない）
(() => {
  if (window.mv) return;
  const send = (message) => { try { dioxus.send(message); } catch (_) {} };
  const blockOf = (node) => node && node.closest && node.closest(".blk");

  // 作業の印。書き出しの印は disabled なので、押されたことは項目（li）で受ける
  document.addEventListener("click", (event) => {
    const link = event.target.closest && event.target.closest("a[href]");
    if (link && blockOf(link)) {
      event.preventDefault();
      const href = link.getAttribute("href");
      if (href.startsWith("#")) {
        const target = document.getElementById(decodeURIComponent(href.slice(1)));
        if (target) target.scrollIntoView({ block: "start" });
      } else {
        send({ kind: "link", href });
      }
      return;
    }
    const item = event.target.closest && event.target.closest("li");
    const block = blockOf(item);
    if (!item || !block || document.getElementById("shell").classList.contains("edit-mode")) return;
    const box = item.querySelector(":scope > input[type=checkbox], :scope > p > input[type=checkbox]");
    if (!box) return;
    // 印のそば（右へ 16 px まで）を押したときだけ。本文を押しただけでは入れ替えない
    if (event.clientX > box.getBoundingClientRect().right + 16) return;
    const all = Array.from(block.querySelectorAll("input[type=checkbox]"));
    send({ kind: "task", block: Number(block.dataset.i), index: all.indexOf(box) });
  }, true);

  // 長押し（Android は contextmenu、iOS は自前で数える）でそのブロックを直す
  document.addEventListener("contextmenu", (event) => {
    const block = blockOf(event.target);
    if (!block) return;
    event.preventDefault();
    send({ kind: "edit", block: Number(block.dataset.i) });
  });
  let press = null;
  document.addEventListener("touchstart", (event) => {
    const block = blockOf(event.target);
    if (!block || event.touches.length !== 1) return;
    const touch = event.touches[0];
    press = { x: touch.clientX, y: touch.clientY, timer: setTimeout(() => {
      press = null;
      send({ kind: "edit", block: Number(block.dataset.i) });
    }, 550) };
  }, { passive: true });
  const cancel = (event) => {
    if (!press) return;
    const touch = event.touches && event.touches[0];
    if (touch && Math.hypot(touch.clientX - press.x, touch.clientY - press.y) < 10) return;
    clearTimeout(press.timer);
    press = null;
  };
  document.addEventListener("touchmove", cancel, { passive: true });
  document.addEventListener("touchend", () => { if (press) { clearTimeout(press.timer); press = null; } }, { passive: true });

  // 道具の帯。**焦点を外さない**（押し始めを止める）
  const keep = (event) => { if (event.target.closest && event.target.closest(".tools button")) event.preventDefault(); };
  document.addEventListener("pointerdown", keep, true);
  document.addEventListener("mousedown", keep, true);

  let composing = false;
  document.addEventListener("compositionstart", () => { composing = true; }, true);
  document.addEventListener("compositionend", () => { composing = false; }, true);

  const field = () => document.getElementById("block-src") || document.getElementById("full-src");
  const lineStart = (text, at) => text.lastIndexOf("\n", at - 1) + 1;
  const insert = (area, text) => {
    // execCommand で入れると、取り消し（undo）の履歴に残る
    if (!document.execCommand("insertText", false, text)) {
      area.setRangeText(text, area.selectionStart, area.selectionEnd, "end");
      area.dispatchEvent(new Event("input", { bubbles: true }));
    }
  };
  const prefix = (area, mark) => {
    const start = lineStart(area.value, area.selectionStart);
    const caret = area.selectionStart;
    const line = area.value.slice(start, area.value.indexOf("\n", start) < 0 ? undefined : area.value.indexOf("\n", start));
    area.setSelectionRange(start, start);
    if (line.startsWith(mark)) {
      area.setSelectionRange(start, start + mark.length);
      insert(area, "");
      area.setSelectionRange(Math.max(start, caret - mark.length), Math.max(start, caret - mark.length));
    } else {
      insert(area, mark);
      area.setSelectionRange(caret + mark.length, caret + mark.length);
    }
  };
  const wrap = (area, open, close, empty) => {
    const { selectionStart: start, selectionEnd: end } = area;
    const inner = area.value.slice(start, end) || empty;
    insert(area, open + inner + close);
    area.setSelectionRange(start + open.length, start + open.length + inner.length);
  };
  const heading = (area) => {
    const start = lineStart(area.value, area.selectionStart);
    const hashes = (area.value.slice(start).match(/^#{1,6} /) || [""])[0];
    // # → ## → ### → 無し
    const next = hashes.length === 0 ? "# " : hashes.length >= 4 ? "" : "#" + hashes;
    area.setSelectionRange(start, start + hashes.length);
    insert(area, next);
  };

  // 探す（v3.1 R-06）。[始まり, 終わり] の並びを返す。正規表現が読めなければ null（Rust が理由を出す）
  const matcher = (query, opts) => {
    if (opts.regex) {
      let re;
      try { re = new RegExp(query, "gmu" + (opts.caseSensitive ? "" : "i")); } catch (_) { return null; }
      return (text) => {
        const out = [];
        re.lastIndex = 0;
        for (let m = re.exec(text); m; m = re.exec(text)) {
          // 幅 0 の一致は数えない（土台の検索と同じ）
          if (m[0].length === 0) { re.lastIndex++; continue; }
          out.push([m.index, m.index + m[0].length]);
        }
        return out;
      };
    }
    const needle = opts.caseSensitive ? query : query.toLowerCase();
    return (data) => {
      const text = opts.caseSensitive ? data : data.toLowerCase();
      const out = [];
      for (let at = text.indexOf(needle); at >= 0; at = text.indexOf(needle, at + needle.length)) {
        out.push([at, at + needle.length]);
      }
      return out;
    };
  };
  // 語の区切り。**土台の search::word と同じ決まり**（字の種類が変わる所で区切る。記号の隣はいつも切れている）
  const kindOf = (ch) => {
    if (/\s/u.test(ch)) return "space";
    if (/[\p{Alphabetic}\p{N}]/u.test(ch)) {
      if (/[ぁ-ゖー]/u.test(ch)) return "hiragana";
      if (/[ァ-ヺｦ-ﾝ]/u.test(ch)) return "katakana";
      if (/[一-鿿々〆]/u.test(ch)) return "han";
      return "alnum";
    }
    return "symbol";
  };
  const charBefore = (text, at) => {
    if (at <= 0) return null;
    const code = text.charCodeAt(at - 1);
    return code >= 0xdc00 && code <= 0xdfff && at >= 2 ? text.slice(at - 2, at) : text[at - 1];
  };
  const charAt = (text, at) => (at >= text.length ? null : String.fromCodePoint(text.codePointAt(at)));
  const wholeWord = (text, start, end) => {
    const joins = (neighbor, edge) => {
      const kind = kindOf(edge);
      return kind !== "symbol" && kind !== "space" && neighbor !== null && kindOf(neighbor) === kind;
    };
    return !joins(charBefore(text, start), charAt(text, start)) && !joins(charAt(text, end), charBefore(text, end));
  };

  window.mv = {
    tool(name) {
      const area = field();
      if (!area || composing) return;
      area.focus();
      switch (name) {
        case "undo": document.execCommand("undo"); break;
        case "left": area.setSelectionRange(Math.max(0, area.selectionStart - 1), Math.max(0, area.selectionStart - 1)); break;
        case "right": area.setSelectionRange(area.selectionEnd + 1, area.selectionEnd + 1); break;
        case "heading": heading(area); break;
        case "bullet": prefix(area, "- "); break;
        case "task": prefix(area, "- [ ] "); break;
        case "bold": wrap(area, "**", "**", "太字"); break;
        case "code": wrap(area, "`", "`", "code"); break;
        case "link": wrap(area, "[", "](https://)", "リンク"); break;
        case "indent": prefix(area, "  "); break;
      }
    },
    focus(id) {
      const area = document.getElementById(id);
      if (area) { area.focus(); area.setSelectionRange(area.value.length, area.value.length); }
    },
    // スライド（v3.1 R-10）。n 枚目へ・いま見ている枚から d 枚送る
    scrollToSlide(n) {
      const slide = document.querySelector('[data-slide="' + n + '"]');
      if (slide) slide.scrollIntoView({ block: "start" });
    },
    stepSlide(d) {
      const read = document.getElementById("read");
      const slides = Array.from(document.querySelectorAll("[data-slide]"));
      if (!read || !slides.length) return;
      const top = read.getBoundingClientRect().top + parseFloat(getComputedStyle(read).paddingTop || "0");
      // いま見ている枚 = 半分より下が画面に残っている、いちばん上の枚
      let now = slides.findIndex((slide) => slide.getBoundingClientRect().bottom > top + slide.getBoundingClientRect().height / 2);
      if (now < 0) now = slides.length - 1;
      const next = Math.max(0, Math.min(slides.length - 1, now + d));
      slides[next].scrollIntoView({ block: "start" });
    },
    scrollTo(index) {
      const block = document.querySelector('.blk[data-i="' + index + '"]');
      if (block) block.scrollIntoView({ block: "start" });
    },
    // 検索の印（v3.1 R-06: PC 版と同じ 3 つの指定。opts = { caseSensitive, regex, wholeWord }）。
    // 探すのは描いた字（PC 版のプレビューの印と同じ）。置換は Rust が原文を土台の検索で置き換える
    mark(query, current, opts) {
      const marks = window.CSS && CSS.highlights;
      if (marks) { marks.delete("mv-hit"); marks.delete("mv-current"); }
      if (!query) { send({ kind: "found", count: 0 }); return; }
      opts = opts || {};
      const finder = matcher(query, opts);
      if (!finder) { send({ kind: "found", count: 0 }); return; }
      const hits = [];
      let currentRange = null;
      const walker = document.createTreeWalker(document.getElementById("read"), NodeFilter.SHOW_TEXT);
      let count = 0;
      for (let node = walker.nextNode(); node && hits.length < 2000; node = walker.nextNode()) {
        for (const [at, end] of finder(node.data)) {
          if (opts.wholeWord && !wholeWord(node.data, at, end)) continue;
          const range = new Range();
          range.setStart(node, at);
          range.setEnd(node, end);
          if (count === current) currentRange = range; else hits.push(range);
          count++;
        }
      }
      send({ kind: "found", count });
      if (marks) marks.set("mv-hit", new Highlight(...hits));
      if (currentRange) {
        if (marks) marks.set("mv-current", new Highlight(currentRange));
        const rect = currentRange.getBoundingClientRect();
        const read = document.getElementById("read");
        if (rect.top < 140 || rect.bottom > read.clientHeight - 80) {
          read.scrollTop += rect.top - read.clientHeight / 3;
        }
      }
    },
  };

  // Back キー（v3.1 R-01）。**見張りの履歴を 1 つ積んでおく**。Android の WryActivity は、WebView に戻れる履歴が
  // あれば Back で戻り（ここで popstate になる）、無ければアプリを閉じる。戻ったら Rust に知らせ、Rust が開いている
  // ものを 1 段閉じて mv.arm() で積み直す。何も開いていなければ 2 秒だけ積み直さず、その間の Back でアプリが閉じる
  const arm = () => { history.pushState({ mv: "guard" }, ""); };
  window.addEventListener("popstate", () => send({ kind: "back" }));
  window.mv.arm = arm;
  arm();

  // 読んでいる所（v3.1 R-09）。スクロールが止まったら、画面のいちばん上に見えているブロックの番号を知らせる
  let settle = null;
  document.addEventListener("scroll", (event) => {
    const read = event.target;
    if (!read || read.id !== "read") return;
    clearTimeout(settle);
    settle = setTimeout(() => {
      const top = read.getBoundingClientRect().top + parseFloat(getComputedStyle(read).paddingTop || "0");
      const first = Array.from(read.querySelectorAll(".blk")).find((blk) => blk.getBoundingClientRect().bottom > top + 1);
      if (first) send({ kind: "position", block: Number(first.dataset.i) });
    }, 300);
  }, true);

  // OS の明るさ（v3.1 R-04）。テーマが「OS に従う」のときに使う。変わったら知らせ直す
  const scheme = window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)");
  if (scheme) {
    const tell = () => send({ kind: "scheme", dark: scheme.matches });
    if (scheme.addEventListener) scheme.addEventListener("change", tell); else scheme.addListener(tell);
    tell();
  }

  // キーボードの高さ（下から出る板と検索の帯を、キーボードの上へ上げる）
  const viewport = window.visualViewport;
  if (viewport) {
    const update = () => {
      const covered = Math.max(0, window.innerHeight - viewport.height - viewport.offsetTop);
      document.documentElement.style.setProperty("--kb", covered + "px");
    };
    viewport.addEventListener("resize", update);
    viewport.addEventListener("scroll", update);
    update();
  }
  send({ kind: "ready" });
})();
