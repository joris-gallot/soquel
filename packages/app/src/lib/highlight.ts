import type { Parser } from '@lezer/common'
import { jsonLanguage } from '@codemirror/lang-json'
import { PostgreSQL, sql } from '@codemirror/lang-sql'
import { classHighlighter, highlightCode } from '@lezer/highlight'

function escapeHtml(text: string): string {
  return text
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
}

/// Code as HTML with `tok-*` classes (styled in style.css), no editor instance.
function highlightWith(parser: Parser, code: string): string {
  let html = ''
  highlightCode(
    code,
    parser.parse(code),
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

export function highlightJson(code: string): string {
  return highlightWith(jsonLanguage.parser, code)
}

const sqlParser = sql({ dialect: PostgreSQL }).language.parser

export function highlightSql(code: string): string {
  return highlightWith(sqlParser, code)
}
