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
    scrollTo(index) {
      const block = document.querySelector('.blk[data-i="' + index + '"]');
      if (block) block.scrollIntoView({ block: "start" });
    },
    mark(query, current) {
      const marks = window.CSS && CSS.highlights;
      if (marks) { marks.delete("mv-hit"); marks.delete("mv-current"); }
      if (!query) { send({ kind: "found", count: 0 }); return; }
      const needle = query.toLowerCase();
      const hits = [];
      let currentRange = null;
      const walker = document.createTreeWalker(document.getElementById("read"), NodeFilter.SHOW_TEXT);
      let count = 0;
      for (let node = walker.nextNode(); node && hits.length < 2000; node = walker.nextNode()) {
        const text = node.data.toLowerCase();
        for (let at = text.indexOf(needle); at >= 0; at = text.indexOf(needle, at + needle.length)) {
          const range = new Range();
          range.setStart(node, at);
          range.setEnd(node, at + needle.length);
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
        if (rect.top < 80 || rect.bottom > read.clientHeight - 40) {
          read.scrollTop += rect.top - read.clientHeight / 3;
        }
      }
    },
  };

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
