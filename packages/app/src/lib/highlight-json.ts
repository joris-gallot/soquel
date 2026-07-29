import { jsonLanguage } from '@codemirror/lang-json'
import { classHighlighter, highlightCode } from '@lezer/highlight'

function escapeHtml(text: string): string {
  return text
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
}

/// Pretty-printed json as HTML with `tok-*` classes (styled in style.css).
export function highlightJson(code: string): string {
  let html = ''
  highlightCode(
    code,
    jsonLanguage.parser.parse(code),
    classHighlighter,
    (text, classes) => {
      html += classes === ''
        ? escapeHtml(text)
        : `<span class="${classes}">${escapeHtml(text)}</span>`
    },
    () => {
      html += '\n'
    },
  )
  return html
}
