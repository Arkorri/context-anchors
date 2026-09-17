// Loaded after mdBook's own scripts (`additional-js` in @ref[site/book.toml]). Two jobs.
//
// 1. The theme menu: @ref[./custom.css] keeps two of mdBook's five theme slots and gives them the
//    site's palettes; the dark one lives in the `ayu` slot, so its menu entry is relabelled.
//
// 2. Example output: the guide shows what `anchr check` prints, in plain text blocks. In a
//    terminal that output is coloured; here the same colouring is applied by line shape, so
//    the examples read as the real thing. Only blocks that look like a report are touched.

(function () {
  const dark = document.getElementById("mdbook-theme-ayu");
  if (dark) dark.textContent = "Dark";

  const REPORT = /^(error|warning|unverified): |^ *--> /m;
  for (const code of document.querySelectorAll("pre > code.language-text")) {
    const text = code.textContent;
    if (!REPORT.test(text)) continue;
    const caret = /^error: /m.test(text) ? "diag-error" : "diag-warning";
    const lines = text.replace(/\n$/, "").split("\n");
    code.innerHTML = lines.map((line) => colour(line, caret)).join("\n") + "\n";
  }

  function colour(line, caret) {
    let m;
    if ((m = line.match(/^(error|warning|unverified)(: .*)$/))) {
      const kind = m[1] === "error" ? "diag-error" : "diag-warning";
      return span(kind, m[1]) + escape(m[2]);
    }
    if ((m = line.match(/^( *)(-->|:::)(.*)$/))) {
      return m[1] + span("diag-arrow", m[2]) + escape(m[3]);
    }
    if ((m = line.match(/^( *\| *)(\^+)(.*)$/))) {
      return span("diag-gutter", m[1]) + span(`diag-caret ${caret}`, m[2]) + escape(m[3]);
    }
    if ((m = line.match(/^( *= )(help|note)(:.*)$/))) {
      return span("diag-gutter", m[1]) + span("diag-help", m[2]) + escape(m[3]);
    }
    if ((m = line.match(/^( *\d* *\|)(.*)$/))) {
      return span("diag-gutter", m[1]) + escape(m[2]);
    }
    if (/^checked \d+ references/.test(line)) {
      return span("diag-summary", line);
    }
    return escape(line);
  }

  function span(cls, text) {
    return `<span class="${cls}">${escape(text)}</span>`;
  }

  function escape(text) {
    return text.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
  }
})();
