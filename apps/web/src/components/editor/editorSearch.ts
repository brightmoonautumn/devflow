import { runScopeHandlers, type EditorView, type Panel } from '@codemirror/view'
import { closeSearchPanel, findNext, findPrevious, getSearchQuery, replaceAll, replaceNext, search, SearchQuery, selectMatches, setSearchQuery } from '@codemirror/search'

function button(text: string, title: string, action: () => void, className = '') {
  const element = document.createElement('button')
  element.type = 'button'
  element.className = `df-search-button ${className}`
  element.textContent = text
  element.title = title
  element.setAttribute('aria-label', title)
  element.addEventListener('click', action)
  return element
}

function field(placeholder: string, name: string) {
  const input = document.createElement('input')
  input.className = 'df-search-input'
  input.name = name
  input.placeholder = placeholder
  input.setAttribute('aria-label', placeholder)
  input.setAttribute('form', '')
  input.autocomplete = 'off'
  input.spellcheck = false
  return input
}

/** Adapt only the panel UI; CodeMirror owns queries, navigation and replacements. */
class EditorSearchPanel implements Panel {
  readonly dom = document.createElement('div')
  readonly top = true
  readonly pos = 80
  private query: SearchQuery
  private searchField = field('查找', 'search')
  private replaceField = field('替换为', 'replace')
  private replaceRow = document.createElement('div')
  private expanded = true
  private toggle: HTMLButtonElement
  private options: { element: HTMLButtonElement; key: 'caseSensitive' | 'wholeWord' | 'regexp' }[]

  constructor(private view: EditorView) {
    this.query = getSearchQuery(view.state)
    this.dom.className = 'df-search-panel'
    this.dom.setAttribute('role', 'search')
    this.dom.setAttribute('aria-label', '文件内查找替换')
    this.searchField.setAttribute('main-field', 'true')

    this.toggle = button('⌄', '收起替换', () => {
      this.expanded = !this.expanded
      this.syncRows()
      this.view.requestMeasure()
    }, 'df-search-toggle')
    const close = button('×', '关闭查找（Esc）', () => { closeSearchPanel(view); view.focus() }, 'df-search-close')
    const row = document.createElement('div')
    row.className = 'df-search-row'
    const inputGroup = document.createElement('div')
    inputGroup.className = 'df-search-field'
    const label = document.createElement('span')
    label.className = 'df-search-label'
    label.textContent = '查找'
    label.setAttribute('aria-hidden', 'true')
    inputGroup.append(label, this.searchField)
    this.options = ([
      { key: 'caseSensitive', text: '大小写', title: '区分大小写' },
      { key: 'wholeWord', text: '全字', title: '全字匹配' },
      { key: 'regexp', text: '正则', title: '正则表达式' },
    ] as const).map(({ key, text, title }) => {
      const element = button(text, title, () => this.commit({ [key]: !this.query[key] }), 'df-search-option')
      inputGroup.append(element)
      return { element, key }
    })
    const actions = document.createElement('div')
    actions.className = 'df-search-actions'
    actions.append(
      button('↑', '上一个匹配（Shift+Enter）', () => { findPrevious(view) }, 'df-search-arrow'),
      button('↓', '下一个匹配（Enter）', () => { findNext(view) }, 'df-search-arrow'),
      button('全选', '全选匹配', () => { selectMatches(view) }),
    )
    row.append(inputGroup, actions)

    this.replaceRow.className = 'df-search-row df-search-replace-row'
    const replaceGroup = document.createElement('div')
    replaceGroup.className = 'df-search-field'
    const replaceLabel = label.cloneNode() as HTMLSpanElement
    replaceLabel.textContent = '替换'
    replaceGroup.append(replaceLabel, this.replaceField)
    const replaceActions = document.createElement('div')
    replaceActions.className = 'df-search-actions'
    replaceActions.append(
      button('替换', '替换当前匹配', () => { replaceNext(view) }),
      button('全部替换', '替换全部匹配', () => { replaceAll(view) }),
    )
    this.replaceRow.append(replaceGroup, replaceActions)
    this.dom.append(this.toggle, row, this.replaceRow, close)
    this.searchField.addEventListener('input', () => this.commit())
    this.replaceField.addEventListener('input', () => this.commit())
    this.dom.addEventListener('keydown', event => {
      if (event.isComposing) return
      if (runScopeHandlers(view, event, 'search-panel')) event.preventDefault()
      else if (event.key === 'Enter' && event.target === this.searchField) {
        event.preventDefault()
        const navigate = event.shiftKey ? findPrevious : findNext
        navigate(view)
      } else if (event.key === 'Enter' && event.target === this.replaceField) {
        event.preventDefault()
        replaceNext(view)
      }
    })
    this.sync()
  }

  private commit(options: Partial<Pick<SearchQuery, 'caseSensitive' | 'wholeWord' | 'regexp'>> = {}) {
    const query = new SearchQuery({
      search: this.searchField.value, replace: this.replaceField.value,
      caseSensitive: this.query.caseSensitive, wholeWord: this.query.wholeWord,
      regexp: this.query.regexp, literal: this.query.literal, test: this.query.test,
      ...options,
    })
    if (!query.eq(this.query)) this.view.dispatch({ effects: setSearchQuery.of(query) })
  }

  private syncRows() {
    this.toggle.hidden = this.view.state.readOnly
    this.replaceRow.hidden = this.view.state.readOnly || !this.expanded
    this.toggle.textContent = this.expanded ? '⌄' : '›'
    const title = this.expanded ? '收起替换' : '展开替换'
    this.toggle.title = title
    this.toggle.setAttribute('aria-label', title)
    this.toggle.setAttribute('aria-expanded', String(this.expanded))
  }

  private sync() {
    this.query = getSearchQuery(this.view.state)
    if (this.searchField.value !== this.query.search) this.searchField.value = this.query.search
    if (this.replaceField.value !== this.query.replace) this.replaceField.value = this.query.replace
    this.searchField.setAttribute('aria-invalid', String(!this.query.valid && !!this.query.search))
    for (const { element, key } of this.options) element.setAttribute('aria-pressed', String(this.query[key]))
    this.syncRows()
  }

  update() { this.sync() }
  mount() { this.searchField.select() }
}

export const editorSearch = search({ top: true, createPanel: view => new EditorSearchPanel(view) })
