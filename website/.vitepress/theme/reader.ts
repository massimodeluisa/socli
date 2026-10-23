export interface Metrics {
  replies?: number
  reposts?: number
  likes?: number
  views?: string
}

export interface LinkCard {
  host: string
  title: string
  description: string
}

export interface Post {
  id: string
  name: string
  handle: string
  net: string
  ago: string
  color: string
  text: string
  metrics: Metrics
  replies: string[]
  link?: LinkCard
  image?: boolean
}

interface Cell {
  t: string
  c: string
}

interface TextRow {
  kind?: undefined
  cells: Cell[]
  sel: boolean
  id: string
}

interface RuleRow {
  kind: 'rule'
}

interface CardRow {
  kind: 'card'
  sel: boolean
  id: string
  color: string
  host?: string
  title?: string
  description?: string
  image?: boolean
  alt?: string
}

type FeedRow = TextRow | RuleRow | CardRow

interface AvatarOptions {
  up?: boolean
  down?: boolean
  join?: boolean
}

interface ReplyOptions {
  up: boolean
  down: boolean
  lastElbow: boolean
  rootColor: string
  blockId: string
}

interface ReaderState {
  view: 'feed' | 'thread' | 'help'
  helpReturn: 'feed' | 'thread'
  index: number
  thread: string
  threadIndex: number
  liked: Set<string>
  pinned: string
}

const COLS = 92
const HOME = ['p1', 'p2', 'p8', 'p11', 'p10']

const RICH_RE = /https?:\/\/\S+|@[A-Za-z0-9.]+(@[A-Za-z0-9.]+)?/g

function post(
  id: string,
  name: string,
  handle: string,
  net: string,
  ago: string,
  color: string,
  text: string,
  metrics: Metrics,
  replies: string[],
  extra?: { link?: LinkCard; image?: boolean },
): Post {
  return { id, name, handle, net, ago, color, text, metrics, replies, ...(extra ?? {}) }
}

const POSTS: Record<string, Post> = {
  p1: post(
    'p1',
    'Mina Cho',
    'mina',
    'x',
    '3h',
    'teal',
    'Step 1: draw the thread in the terminal.\nStep 2: leave the scrollback alone.\n\n… @nuno if you know what I mean',
    { replies: 6, reposts: 4, likes: 48, views: '2.1K' },
    [],
  ),
  p2: post(
    'p2',
    'Nuno Park',
    'nuno',
    'devnull',
    '50m',
    'amber',
    'ok devs, assemble. the reader is up.\n\nnotes: https://example.com/socli\n\nmake some noise.',
    { replies: 5, reposts: 0, likes: 86 },
    ['p3', 'p4', 'p5', 'p6', 'p7'],
    {
      link: {
        host: 'example.com/socli',
        title: 'socli',
        description:
          'A thread-first terminal client for X, Bluesky, dev/null, Threads and Mastodon',
      },
    },
  ),
  p3: post('p3', 'Priya Shah', 'priya', 'devnull', '40m', 'teal', 'Hype', { likes: 4 }, []),
  p4: post(
    'p4',
    'Juri Holm',
    'juri',
    'devnull',
    '28m',
    'coral',
    'The intro thread is the whole product.',
    { likes: 9 },
    [],
  ),
  p5: post(
    'p5',
    'Diego Martins',
    'diego',
    'devnull',
    '20m',
    'amber',
    'Those were the fastest 18 hours of the build.',
    { likes: 15 },
    [],
  ),
  p6: post(
    'p6',
    'Sam Idris',
    'sam',
    'devnull',
    '12m',
    'rose',
    'Image next. Kitty, iTerm and sixel terminals draw it; the rest get half blocks.',
    { likes: 6 },
    [],
  ),
  p7: post(
    'p7',
    'Lea Ortega',
    'lea',
    'devnull',
    '7m',
    'coral',
    'The rails are the interface.',
    { likes: 3 },
    [],
  ),
  p8: post(
    'p8',
    'Ada Okonkwo',
    'ada.bsky.social',
    'bluesky',
    '5h',
    'gold',
    'shipped a sparkline so the reader has something to look at.\n\npress v.',
    { replies: 1, reposts: 2, likes: 21 },
    ['p9'],
    { image: true },
  ),
  p9: post(
    'p9',
    'Mina Cho',
    'mina.bsky.social',
    'bluesky',
    '4h',
    'teal',
    'v opens it full screen. without image support, the same library paints half blocks.',
    { likes: 2 },
    [],
  ),
  p10: post(
    'p10',
    'Kal Nguyen',
    'kal',
    'threads',
    '8h',
    'amber',
    'Reading this in a terminal on purpose.',
    {},
    [],
  ),
  p11: post(
    'p11',
    'Ines Duarte',
    'ines@mastodon.social',
    'mastodon',
    '40m',
    'rose',
    'Boosts show who shared a post above it, the way the web app does. @theo@fosstodon.org was right about full handles.',
    { replies: 1, reposts: 3, likes: 12 },
    ['r11'],
  ),
  r11: post(
    'r11',
    'Theo Lang',
    'theo@fosstodon.org',
    'mastodon',
    '32m',
    'teal',
    'Full handles look long, but they tell you which server answers.',
    { likes: 2 },
    [],
  ),
}

const NET: Record<string, { label: string; cls: string }> = {
  x: { label: 'x', cls: '' },
  bluesky: { label: 'bluesky', cls: 'sky' },
  devnull: { label: 'dev/null', cls: 'amber' },
  threads: { label: 'threads', cls: 'violet' },
  mastodon: { label: 'mastodon', cls: 'indigo' },
}

const HELP: Array<[string, Array<[string, string]>]> = [
  [
    'Move',
    [
      ['j k  ↓ ↑', 'move'],
      ['g G  Home End', 'first / last'],
      ['⏎', 'open'],
      ['esc', 'back'],
    ],
  ],
  [
    'Act',
    [
      ['l', 'like, again to undo'],
      ['r', 'reply'],
      ['n', 'new post'],
      ['v', 'view images'],
      ['m', 'load more'],
    ],
  ],
  [
    'Other',
    [
      ['?', 'this help'],
      ['q', 'quit'],
    ],
  ],
]

export function mountReader(root: HTMLElement): () => void {
  const screen = root.querySelector<HTMLElement>('#screen')
  const feed = root.querySelector<HTMLElement>('#feed')
  const top = root.querySelector<HTMLElement>('#chrome-top')
  const bot = root.querySelector<HTMLElement>('#chrome-bot')
  const live = root.querySelector<HTMLElement>('#live')
  const term = root.querySelector<HTMLElement>('#term')

  if (!screen || !feed || !top || !bot || !live || !term) {
    return () => {}
  }

  const state: ReaderState = {
    view: 'feed',
    helpReturn: 'feed',
    index: 1,
    thread: '',
    threadIndex: 0,
    liked: new Set(),
    pinned: '',
  }

  const controller = new AbortController()
  const { signal } = controller
  const timers = new Set<ReturnType<typeof setTimeout>>()

  function byId(id: string): Post {
    const item = POSTS[id]
    if (!item) throw new Error(`unknown post ${id}`)
    return item
  }

  function blank(sel: boolean, id?: string): TextRow {
    return {
      cells: Array.from({ length: COLS }, () => ({ t: ' ', c: '' })),
      sel: !!sel,
      id: id ?? '',
    }
  }

  function put(row: TextRow, col: number, text: string, cls?: string): number {
    const chars = Array.from(text)
    for (let i = 0; i < chars.length && col + i < COLS; i++) {
      row.cells[col + i] = { t: chars[i], c: cls ?? '' }
    }
    return chars.length
  }

  function putRight(row: TextRow, text: string, cls?: string): void {
    put(row, Math.max(0, COLS - Array.from(text).length), text, cls)
  }

  function rich(row: TextRow, col: number, text: string): void {
    let at = col
    let last = 0
    for (const match of text.matchAll(RICH_RE)) {
      at += put(row, at, text.slice(last, match.index), '')
      at += put(row, at, match[0], 'blue')
      last = match.index + match[0].length
    }
    put(row, at, text.slice(last), '')
  }

  function paint(el: HTMLElement, rows: FeedRow[]): void {
    const frag = document.createDocumentFragment()
    for (const row of rows) {
      if (row.kind === 'rule') {
        const rule = document.createElement('div')
        rule.className = 'rule'
        frag.append(rule)
        continue
      }
      if (row.kind === 'card') {
        frag.append(cardEl(row))
        continue
      }
      const div = document.createElement('div')
      div.className = 'row' + (row.sel ? ' is-sel' : '')
      if (row.id) div.dataset.id = row.id
      let run: HTMLSpanElement | null = null
      const flush = () => {
        if (run) div.append(run)
      }
      for (const cell of row.cells) {
        if (run && run.dataset.c === cell.c) run.textContent += cell.t
        else {
          flush()
          run = document.createElement('span')
          run.dataset.c = cell.c
          if (cell.c) run.className = cell.c
          run.textContent = cell.t
        }
      }
      flush()
      frag.append(div)
    }
    el.replaceChildren(frag)
  }

  function cardEl(row: CardRow): HTMLElement {
    const wrap = document.createElement('div')
    wrap.className = 'card-row' + (row.sel ? ' is-sel' : '')
    if (row.id) wrap.dataset.id = row.id
    const indent = document.createElement('span')
    indent.className = 'indent'
    const bar = document.createElement('span')
    if (row.sel) bar.className = 'blue'
    bar.textContent = row.sel ? '▍' : ' '
    const rail = document.createElement('span')
    rail.className = row.color
    rail.textContent = '╎'
    indent.append(bar, document.createTextNode('   '), rail, document.createTextNode('    '))
    const card = document.createElement('div')
    card.className = 'card'
    if (row.image) {
      card.append(cardLine('▣ Image', 'v view'))
      card.append(spark())
      card.append(cardLine(row.alt ?? '', ''))
    } else {
      card.append(cardLine('↗ ' + (row.host ?? ''), ''))
      const title = document.createElement('div')
      title.className = 'card-alt'
      const name = document.createElement('span')
      name.className = 'b'
      name.style.color = 'var(--term)'
      name.textContent = row.title ?? ''
      const desc = document.createElement('span')
      desc.textContent = row.description ?? ''
      title.append(name, desc)
      card.append(title)
    }
    wrap.append(indent, card)
    return wrap
  }

  function cardLine(left: string, right: string): HTMLElement {
    const line = document.createElement('div')
    line.className = 'card-h'
    const a = document.createElement('span')
    a.textContent = left
    line.append(a)
    if (right) {
      const b = document.createElement('span')
      b.textContent = right
      line.append(b)
    }
    return line
  }

  function spark(): SVGElement {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    svg.setAttribute('viewBox', '0 0 400 140')
    svg.setAttribute('preserveAspectRatio', 'none')
    svg.setAttribute('aria-hidden', 'true')
    const line =
      'M0.0 95.5 L17.4 87.8 L34.8 91.6 L52.2 76.4 L69.6 80.2 L87.0 66.2 L104.3 71.3 L121.7 58.5 L139.1 62.4 L156.5 49.6 L173.9 56.0 L191.3 44.5 L208.7 52.2 L226.1 39.5 L243.5 42.0 L260.9 33.1 L278.3 36.9 L295.7 28.0 L313.0 31.8 L330.4 24.2 L347.8 29.3 L365.2 19.1 L382.6 21.6 L400.0 16.5'
    svg.innerHTML =
      '<path d="M0 46 H400 M0 92 H400" stroke="var(--rule)" stroke-width="1" fill="none"/>' +
      '<path d="' +
      line +
      ' L400 140 L0 140 Z" fill="var(--sky)" fill-opacity="0.22"/>' +
      '<path d="' +
      line +
      '" fill="none" stroke="var(--sky)" stroke-width="2.4" stroke-linejoin="round"/>' +
      '<circle cx="400" cy="16.5" r="4" fill="var(--sky)"/>'
    return svg
  }

  function focusBar(row: TextRow): void {
    if (row.sel) row.cells[0] = { t: '▍', c: 'blue' }
  }

  function crumbs(row: TextRow, parts: Array<[string, string]>): void {
    let col = 2
    for (const [text, cls] of parts) col += put(row, col, text, cls)
  }

  function header(left: Array<[string, string]>, right?: string): TextRow {
    const row = blank(false)
    crumbs(row, left)
    if (right) putRight(row, right, 'dim')
    return row
  }

  function chips(): TextRow {
    const row = blank(false)
    let col = 2
    const add = (text: string, cls: string) => {
      col += put(row, col, text, cls)
    }
    add('[All]', 'b')
    const nets: Array<[string, string]> = [
      ['X', ''],
      ['Bluesky', 'sky'],
      ['dev/null', 'amber'],
      ['Threads', 'violet'],
      ['Mastodon', 'indigo'],
    ]
    for (const [label, cls] of nets) {
      add('  ', '')
      add('●', cls)
      add(' ' + label, '')
    }
    add('   ', '')
    add('│', 'rulec')
    add('  ', '')
    add('[Home]', 'b')
    add('  Discover  Saved', 'dim')
    return row
  }

  function hints(pairs: Array<[string, string]>): TextRow {
    const row = blank(false)
    let col = 2
    pairs.forEach(([key, label], i) => {
      const gap = i ? '  ' : ''
      const next = col + Array.from(gap + key + ' ' + label).length
      if (next > COLS - 8) return
      col += put(row, col, gap, '')
      col += put(row, col, key, 'b')
      col += put(row, col, ' ' + label, 'dim')
    })
    putRight(row, 'demo', 'dim')
    return row
  }

  function nav(active: string): TextRow {
    const row = blank(false)
    let col = 2
    const add = (text: string, cls: string) => {
      col += put(row, col, text, cls)
    }
    add('socli›', 'b')
    const items = ['Home', 'Notifications', 'Search']
    for (const item of items) {
      add('  ', '')
      if (item === active) add('[' + item + ']', 'b')
      else add(item, 'dim')
    }
    putRight(row, '[n] new post', 'dim')
    return row
  }

  function avatar(letter: string, color: string, opts: AvatarOptions): Cell[][] {
    const top = (opts.up ? '╭─┴─╮' : '╭───╮').split('')
    const mid = (opts.join ? '┤ ' + letter + ' │' : '│ ' + letter + ' │').split('')
    const bot = (opts.down ? '╰─┬─╯' : '╰───╯').split('')
    const paintGlyph = (ch: string) => {
      if (ch === letter) return 'b'
      if (ch === ' ') return ''
      return color
    }
    return [top, mid, bot].map((chars) => chars.map((ch) => ({ t: ch, c: paintGlyph(ch) })))
  }

  function placeAvatar(row: TextRow, col: number, glyphs: Cell[][]): void {
    glyphs.forEach((cell, i) => {
      if (col + i < COLS) row.cells[col + i] = cell
    })
  }

  function statsRow(item: Post, sel: boolean): TextRow {
    const row = blank(sel, item.id)
    const m = item.metrics
    const likes = (m.likes || 0) + (state.liked.has(item.id) ? 1 : 0)
    const liked = state.liked.has(item.id)
    if (item.net !== 'threads') {
      put(row, 9, '↩ ' + (m.replies || 0), 'dim')
      if (item.net !== 'devnull') put(row, 18, '⇄ ' + (m.reposts || 0), 'dim')
      put(row, 27, (liked ? '♥ ' : '♡ ') + likes, liked ? 'pink' : 'dim')
      put(row, 36, '☆', 'dim')
    }
    if (m.views) putRight(row, '◉ ' + m.views, 'dim')
    return row
  }

  function hasStats(item: Post): boolean {
    const m = item.metrics
    return (
      item.net !== 'threads' &&
      ((m.replies || 0) + (m.reposts || 0) + (m.likes || 0) > 0 || !!m.views)
    )
  }

  function bodyLines(item: Post, sel: boolean, startCol: number): TextRow[] {
    const lines = item.text.split('\n')
    return lines.map((line) => {
      const row = blank(sel, item.id)
      rich(row, startCol, line)
      return row
    })
  }

  function railAt(row: TextRow, col: number, color: string): void {
    row.cells[col] = { t: '╎', c: color }
  }

  function rootBlock(item: Post, sel: boolean, withReplies: boolean): FeedRow[] {
    const rows: FeedRow[] = []
    const lines = bodyLines(item, sel, 9)
    const av = avatar(item.name[0], item.color, {
      down: withReplies || lines.length > 2 || item.replies.length > 0,
    })
    const head = blank(sel, item.id)
    placeAvatar(head, 2, av[0])
    let col = 9
    col += put(head, col, item.name, 'b')
    col += put(head, col, ' @' + item.handle + ' · ' + item.ago + ' ', 'dim')
    put(head, col, '· ', 'dim')
    put(head, col + 2, NET[item.net].label, NET[item.net].cls)
    putRight(head, '⋯', 'dim')
    rows.push(head)

    const mid = lines[0] || blank(sel, item.id)
    placeAvatar(mid, 2, av[1])
    rows.push(mid)

    const bot = lines[1] || blank(sel, item.id)
    placeAvatar(bot, 2, av[2])
    rows.push(bot)

    for (const extra of lines.slice(2)) {
      railAt(extra, 4, item.color)
      rows.push(extra)
    }

    if (item.link) {
      const gap = blank(sel, item.id)
      railAt(gap, 4, item.color)
      rows.push(gap)
      rows.push({
        kind: 'card',
        sel,
        id: item.id,
        color: item.color,
        host: item.link.host,
        title: item.link.title,
        description: item.link.description,
      })
    }
    if (item.image) {
      const gap = blank(sel, item.id)
      railAt(gap, 4, item.color)
      rows.push(gap)
      rows.push({
        kind: 'card',
        sel,
        id: item.id,
        color: item.color,
        image: true,
        alt: 'A rising sparkline on a black field.',
      })
    }
    if (hasStats(item)) {
      const gap = blank(sel, item.id)
      railAt(gap, 4, item.color)
      rows.push(gap)
      const stats = statsRow(item, sel)
      railAt(stats, 4, item.color)
      rows.push(stats)
    }
    return rows
  }

  function replyBlock(item: Post, sel: boolean, opts: ReplyOptions): FeedRow[] {
    const id = opts.blockId
    const rows: FeedRow[] = []
    const av = avatar(item.name[0], item.color, { up: opts.up, down: opts.down, join: true })
    const head = blank(sel, id)
    railAt(head, 4, opts.rootColor)
    placeAvatar(head, 8, av[0])
    let col = 15
    col += put(head, col, item.name, 'b')
    put(head, col, ' @' + item.handle + ' · ' + item.ago, 'dim')
    if (state.liked.has(item.id)) putRight(head, '♥', 'pink')
    rows.push(head)

    const lines = item.text.split('\n')
    const mid = blank(sel, id)
    put(mid, 4, opts.lastElbow ? '╰╌╌╌' : '├╌╌╌', opts.rootColor)
    placeAvatar(mid, 8, av[1])
    rich(mid, 15, lines[0] || '')
    rows.push(mid)

    const botRow = blank(sel, id)
    if (!opts.lastElbow) railAt(botRow, 4, opts.rootColor)
    placeAvatar(botRow, 8, av[2])
    if (lines[1]) rich(botRow, 15, lines[1])
    rows.push(botRow)

    lines.slice(2).forEach((line) => {
      const extra = blank(sel, id)
      railAt(extra, 10, item.color)
      rich(extra, 15, line)
      rows.push(extra)
    })
    return rows
  }

  function foldRow(sel: boolean, id: string, color: string, count: number): TextRow {
    const row = blank(sel, id)
    put(row, 10, '○', color)
    put(row, 15, 'Show ' + count + ' more replies', color)
    return row
  }

  function showThreadRow(sel: boolean, id: string, color: string): FeedRow[] {
    const row = blank(sel, id)
    railAt(row, 4, color)
    const marker = blank(sel, id)
    put(marker, 4, '○', color)
    put(marker, 9, 'Show this thread', color)
    return [row, marker]
  }

  function tree(id: string, sel: boolean): FeedRow[] {
    const item = byId(id)
    const replies = item.replies.map((rid) => byId(rid))
    const show = replies.length > 3 ? replies.slice(0, 3) : replies
    const hidden = replies.length - show.length
    const rows = rootBlock(
      item,
      sel,
      show.length > 0 || hidden > 0 || (item.metrics.replies || 0) > 0,
    )
    if (show.length) {
      const gap = blank(sel, id)
      railAt(gap, 4, item.color)
      rows.push(gap)
    }
    show.forEach((reply, i) => {
      const last = i === show.length - 1 && hidden === 0
      rows.push(
        ...replyBlock(reply, sel, {
          up: i > 0,
          down: !last,
          lastElbow: i === show.length - 1,
          rootColor: item.color,
          blockId: id,
        }),
      )
    })
    if (hidden) rows.push(foldRow(sel, id, show[show.length - 1].color, hidden))
    else if (!show.length && (item.metrics.replies || 0) > 0) {
      rows.push(...showThreadRow(sel, id, item.color))
    }
    return rows
  }

  function threadRows(rootId: string): FeedRow[] {
    const item = byId(rootId)
    const replies = item.replies.map((id) => byId(id))
    const rows = rootBlock(item, state.threadIndex === 0, replies.length > 0)
    if (replies.length) {
      const gap = blank(state.threadIndex === 0, item.id)
      railAt(gap, 4, item.color)
      rows.push(gap)
    }
    replies.forEach((reply, i) => {
      rows.push(
        ...replyBlock(reply, state.threadIndex === i + 1, {
          up: i > 0,
          down: i < replies.length - 1,
          lastElbow: i === replies.length - 1,
          rootColor: item.color,
          blockId: reply.id,
        }),
      )
    })
    return rows
  }

  function helpRows(): FeedRow[] {
    const rows: FeedRow[] = []
    for (const [label, items] of HELP) {
      items.forEach(([key, what], i) => {
        const row = blank(false)
        if (i === 0) put(row, 2, label, 'b')
        put(row, 12, key, '')
        put(row, 30, what, 'dim')
        rows.push(row)
      })
      rows.push(blank(false))
    }
    const note = blank(false)
    put(note, 2, 'The rest of the list is on this page.', 'dim')
    rows.push(note)
    return rows
  }

  function currentId(): string {
    if (state.view === 'thread') {
      const root = byId(state.thread)
      const list = [state.thread, ...root.replies]
      return list[state.threadIndex]
    }
    return HOME[state.index]
  }

  function listLength(): number {
    if (state.view !== 'thread') return HOME.length
    return 1 + byId(state.thread).replies.length
  }

  function render(): void {
    let body: FeedRow[]
    let head: FeedRow[]
    let foot: FeedRow[]
    if (state.view === 'help') {
      head = [header([['socli', 'b'], [' › ', 'dim'], ['keys', '']])]
      body = helpRows()
      foot = [hints([['any key', 'close']])]
    } else if (state.view === 'thread') {
      const root = byId(state.thread)
      const sub = blank(false)
      put(sub, 2, 'Post', 'b')
      putRight(sub, 'Replies oldest first', 'dim')
      head = [
        header(
          [
            ['socli', 'b'],
            [' › ', 'dim'],
            [NET[root.net].label, NET[root.net].cls],
            [' › ', 'dim'],
            ['thread', ''],
          ],
          'esc back',
        ),
        blank(false),
        sub,
        { kind: 'rule' },
      ]
      body = threadRows(state.thread)
      foot = [
        { kind: 'rule' },
        hints([
          ['j/k', 'move'],
          ['l', 'like'],
          ['esc', 'back'],
          ['?', 'help'],
        ]),
      ]
    } else {
      head = [
        header(
          [
            ['socli', 'b'],
            [' › ', 'dim'],
            ['all', ''],
            [' › ', 'dim'],
            ['home', ''],
          ],
          'demo',
        ),
        blank(false),
        chips(),
        { kind: 'rule' },
      ]
      body = []
      HOME.forEach((id, i) => {
        body.push(...tree(id, i === state.index))
        if (i < HOME.length - 1) body.push(blank(false))
      })
      foot = [
        { kind: 'rule' },
        nav('Home'),
        hints([
          ['j/k', 'move'],
          ['⏎', 'open'],
          ['l', 'like'],
          ['?', 'help'],
        ]),
      ]
    }
    for (const row of body) if (row.kind !== 'rule' && row.kind !== 'card' && row.sel) focusBar(row)
    paint(top, head)
    paint(feed, body)
    paint(bot, foot)
    const id = state.view === 'help' ? '' : currentId()
    if (id) {
      const item = byId(id)
      const pin = state.view + ':' + id
      screen.setAttribute('aria-label', 'socli demo. ' + item.name + ' selected.')
      if (pin !== state.pinned) {
        state.pinned = pin
        const el = feed.querySelector<HTMLElement>('[data-id="' + id + '"]')
        if (el) feed.scrollTop = el.offsetTop
      }
    }
  }

  function move(delta: number): void {
    if (state.view === 'help') return
    const n = listLength()
    if (state.view === 'thread') state.threadIndex = clamp(state.threadIndex + delta, 0, n - 1)
    else state.index = clamp(state.index + delta, 0, n - 1)
    announce()
    render()
  }

  function jump(end: boolean): void {
    if (state.view === 'help') return
    const n = listLength() - 1
    if (state.view === 'thread') state.threadIndex = end ? n : 0
    else state.index = end ? n : 0
    announce()
    render()
  }

  function openSelected(): void {
    if (state.view !== 'feed') return
    const id = HOME[state.index]
    state.thread = id
    state.threadIndex = 0
    state.view = 'thread'
    announce()
    render()
  }

  function back(): void {
    if (state.view === 'thread') {
      state.view = 'feed'
      announce()
      render()
    }
  }

  function like(): void {
    if (state.view === 'help') return
    const item = byId(currentId())
    if (item.net === 'threads') return
    if (state.liked.has(item.id)) state.liked.delete(item.id)
    else state.liked.add(item.id)
    render()
  }

  function announce(): void {
    if (state.view === 'help') return
    const item = byId(currentId())
    live.textContent =
      item.name + (state.view === 'thread' ? ', in the thread.' : ', selected.')
  }

  function clamp(n: number, a: number, b: number): number {
    return Math.max(a, Math.min(b, n))
  }

  function handleKey(key: string): boolean {
    if (state.view === 'help') {
      state.view = state.helpReturn
      render()
      return true
    }
    if (key === '?') {
      state.helpReturn = state.view
      state.view = 'help'
      render()
      return true
    }
    if (key === 'j' || key === 'ArrowDown') move(1)
    else if (key === 'k' || key === 'ArrowUp') move(-1)
    else if (key === 'g' || key === 'Home') jump(false)
    else if (key === 'G' || key === 'End') jump(true)
    else if (key === 'Enter') openSelected()
    else if (key === 'Escape' || key === 'Backspace') back()
    else if (key === 'l') like()
    else return false
    return true
  }

  function onKey(event: KeyboardEvent): void {
    if (event.metaKey || event.ctrlKey || event.altKey) return
    if (handleKey(event.key)) event.preventDefault()
  }

  screen.addEventListener('keydown', onKey, { signal })
  if (term.closest('.stage')) {
    window.addEventListener(
      'keydown',
      (event) => {
        if (!term.matches(':hover')) return
        if (document.activeElement instanceof Node && screen.contains(document.activeElement)) return
        const target = event.target
        if (target instanceof HTMLElement) {
          if (target.isContentEditable || target.closest('input, textarea, select, [contenteditable]')) return
          if ((event.key === 'Enter' || event.key === ' ') && target.closest('a, button')) return
        }
        onKey(event)
        if (event.defaultPrevented) event.stopPropagation()
      },
      { signal, capture: true },
    )
  }
  screen.addEventListener(
    'focus',
    () => {
      term.classList.add('is-live')
    },
    { signal },
  )
  screen.addEventListener(
    'blur',
    () => {
      term.classList.remove('is-live')
    },
    { signal },
  )
  feed.addEventListener(
    'mousedown',
    () => {
      screen.focus()
    },
    { signal },
  )
  feed.addEventListener(
    'click',
    (event) => {
      const target = event.target as HTMLElement | null
      const row = target?.closest<HTMLElement>('[data-id]')
      if (!row || state.view === 'help') return
      const id = row.dataset.id
      if (!id) return
      if (state.view === 'feed') {
        const index = HOME.indexOf(id)
        if (index < 0) return
        if (index === state.index) openSelected()
        else {
          state.index = index
          announce()
          render()
        }
      } else if (state.view === 'thread') {
        const list = [state.thread, ...byId(state.thread).replies]
        const index = list.indexOf(id)
        if (index >= 0) {
          state.threadIndex = index
          announce()
          render()
        }
      }
    },
    { signal },
  )

  root.querySelectorAll<HTMLButtonElement>('.touch button').forEach((button) => {
    button.addEventListener(
      'click',
      () => {
        screen.focus()
        handleKey(button.dataset.key ?? '')
      },
      { signal },
    )
  })

  render()

  return () => {
    controller.abort()
    timers.forEach((id) => clearTimeout(id))
    timers.clear()
  }
}
