// xml-viewer の画面の動き。
//
// - Rust 側へは `POST /api/{コマンド}` で呼び出す（api 関数）
// - Rust 側からは window.__xmlv の関数が呼ばれる（ファイルのドラッグ）
// - 文書は Blob URL にして、スクリプトを止めた iframe に表示する
//   （repos/docs/story/00-overview.md の D2）。文書の中のリンクは、ここで拾って処理する（D6）
// - 文書のタイトルやファイル名は外から来た値なので、HTML として組み立てず textContent で入れる

'use strict';

const state = {
  /** 開いているもの。新しいものが先頭。Rust の SourceView と同じ形。 */
  sources: [],
  /** 選んでいるもの: { source, page }。page が null なら表示できるページが無い。 */
  current: null,
  /** 表示中の文書の Blob URL。 */
  blobUrl: null,
  /** ページ表示の通し番号。古い応答を捨てるために使う。 */
  showSeq: 0,
};

const $ = (id) => document.getElementById(id);

const ICON_CLOSE = '<svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" fill="none" stroke-width="1.3"/></svg>';
const ICON_OPEN = '<svg viewBox="0 0 13 13" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true"><path d="M5 2.5H2.5v8h8V8M7.5 2.5h3v3M10.5 2.5L6 7"/></svg>';
const ICON_WARN = '<svg viewBox="0 0 13 13" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true"><path d="M6.5 1.5l5.5 10h-11z"/><path d="M6.5 5.5v3M6.5 10v.5"/></svg>';

// ---- Rust 側の呼び出し ----

async function api(command, args = {}) {
  const res = await fetch('/api/' + command, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'X-Xmlv': '1' },
    body: JSON.stringify(args),
  });
  const data = await res.json();
  if (!data.ok) {
    const error = new Error(data.error.message);
    error.kind = data.error.kind;
    throw error;
  }
  return data.data;
}

// ---- 小さな部品 ----

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function iconSpan(className, svg, title) {
  const span = el('span', className);
  span.innerHTML = svg; // 固定の SVG だけを入れる
  if (title) span.title = title;
  return span;
}

let toastTimer;
function toast(text) {
  $('toast-text').textContent = text;
  $('toast').hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { $('toast').hidden = true; }, 15000);
}

function setBusy(on) {
  document.body.classList.toggle('busy', on);
}

function baseName(path) {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : path;
}

function dirName(path) {
  const i = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'));
  return i > 0 ? path.slice(0, i) : '';
}

function formatTime(seconds) {
  const d = new Date(seconds * 1000);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getMonth() + 1}/${d.getDate()} ${d.getHours()}:${pad(d.getMinutes())}`;
}

// ---- 画面の切り替え ----

function showEmpty() {
  state.showSeq += 1;
  clearView();
  state.current = null;
  $('empty').hidden = false;
  $('list').hidden = true;
  $('view').hidden = true;
  document.title = 'XML Viewer';
  $('btn-print').disabled = true;
  renderRecent();
}

function showOpened() {
  $('empty').hidden = true;
  $('list').hidden = false;
  $('view').hidden = false;
}

function clearView() {
  $('frame-host').replaceChildren();
  if (state.blobUrl) URL.revokeObjectURL(state.blobUrl);
  state.blobUrl = null;
  $('message').hidden = true;
  $('message').replaceChildren();
}

function showMessage(title, lines, details) {
  clearView();
  const box = el('div');
  box.append(el('h2', null, title));
  for (const line of lines) box.append(el('p', null, line));
  if (details) {
    const dl = el('dl');
    for (const [term, value] of details) {
      if (value) dl.append(el('dt', null, term), el('dd', null, value));
    }
    box.append(dl);
  }
  $('message').append(box);
  $('message').hidden = false;
  $('btn-print').disabled = true;
}

// ---- 開く ----

async function openPaths(paths) {
  if (!paths || !paths.length) return;
  setBusy(true);
  let results;
  try {
    results = (await api('open', { paths })).results;
  } catch (e) {
    toast('開けませんでした: ' + e.message);
    return;
  } finally {
    setBusy(false);
  }

  const failures = [];
  const notes = [];
  let last = null;
  for (const r of results) {
    if (!r.ok) {
      failures.push(`${r.path}（${r.error.message}）`);
      continue;
    }
    const s = r.source;
    state.sources = [s, ...state.sources.filter((x) => x.id !== s.id)];
    last = s;
    if (s.truncated) notes.push(`${s.name}: ページが多いため、${s.pages.length} 件だけを一覧に出しています。`);
    if (s.skippedEntries) notes.push(`${s.name}: 名前に問題がある ${s.skippedEntries} 件のファイルは展開しませんでした。`);
  }

  if (last) {
    showOpened();
    select(last);
  } else if (!state.sources.length) {
    showEmpty();
  }
  const messages = [];
  if (failures.length) messages.push('開けませんでした: ' + failures.join('、'));
  messages.push(...notes);
  if (messages.length) toast(messages.join('\n'));
}

async function pickAndOpen() {
  try {
    const { paths } = await api('pick');
    await openPaths(paths);
  } catch (e) {
    toast(e.message);
  }
}

/** 開いたものを選ぶ。最初に出すページがあれば表示する。 */
function select(source) {
  const page = source.initialPage ?? null;
  if (page !== null) {
    showPage(source.id, page);
    return;
  }
  state.showSeq += 1;
  state.current = { source: source.id, page: null };
  renderList();
  document.title = `${source.name} - XML Viewer`;
  if (source.kind === 'xml') {
    const lines = [`${source.name} には、表示用のスタイルシート（XSL）の指定がありません。`];
    if (source.pages.length) lines.push('同じフォルダのほかのページは、左の一覧から表示できます。');
    showMessage('このファイルは表示できません', lines);
  } else {
    const lines = ['表示できるページ（XML と XSL の組）がありません。'];
    if (source.listOthers && source.others.length) lines.push('その他のファイルは、左の一覧から既定のアプリで開けます。');
    showMessage('表示できるページがありません', lines);
  }
}

async function closeSource(id) {
  try {
    await api('close', { source: id });
  } catch (e) {
    toast(e.message);
  }
  state.sources = state.sources.filter((s) => s.id !== id);
  if (state.current && state.current.source === id) {
    clearView();
    if (state.sources.length) select(state.sources[0]);
    else showEmpty();
  } else {
    renderList();
  }
}

// ---- 一覧 ----

function renderList() {
  $('list').replaceChildren(...state.sources.map(renderSource));
}

function renderSource(s) {
  const section = el('section', 'bundle');
  const head = el('div', 'bundle-head');
  const name = el('div', 'bundle-name');
  const showOthers = s.listOthers && s.others.length > 0;
  name.append(el('strong', null, s.name), el('span', null, `${s.pages.length} ページ${showOthers ? `、ほか ${s.others.length} 件` : ''}`));
  const close = el('button', 'icon-btn');
  close.type = 'button';
  close.dataset.close = s.id;
  close.setAttribute('aria-label', `${s.name} を閉じる`);
  close.title = '閉じる';
  close.innerHTML = ICON_CLOSE;
  head.append(name, close);
  section.append(head);

  // フォルダが複数あるときだけ、フォルダ名で区切る（その他のファイルにもフォルダ名を添える）。
  const folders = new Set([...s.pages, ...(showOthers ? s.others : [])].map((x) => x.folder));
  const showFolders = folders.size > 1;
  let list = el('ul', 'entries');
  let lastFolder = null;
  for (const p of s.pages) {
    if (showFolders && p.folder !== lastFolder) {
      if (list.childElementCount) section.append(list);
      section.append(el('p', 'folder-label', p.folder || '（最上位）'));
      list = el('ul', 'entries');
      lastFolder = p.folder;
    }
    list.append(renderPageEntry(s, p));
  }
  if (list.childElementCount) section.append(list);
  if (s.truncated) section.append(el('p', 'list-note', `ページが多いため、${s.pages.length} 件だけを出しています。`));

  if (showOthers) {
    section.append(el('p', 'others-label', 'その他のファイル'));
    const others = el('ul', 'entries');
    for (const o of s.others) {
      const li = el('li');
      const button = el('button', 'entry');
      button.type = 'button';
      button.dataset.source = s.id;
      button.dataset.other = o.index;
      button.title = '既定のアプリで開きます';
      const text = el('span');
      text.append(el('span', 'name', o.file));
      if (showFolders) text.append(el('span', 'file', o.folder || '（最上位）'));
      button.append(el('span', 'mark'), text, iconSpan('tail', ICON_OPEN));
      li.append(button);
      others.append(li);
    }
    section.append(others);
  }
  return section;
}

function renderPageEntry(s, p) {
  const li = el('li');
  const button = el('button', 'entry');
  button.type = 'button';
  button.dataset.source = s.id;
  button.dataset.page = p.index;
  const current = state.current && state.current.source === s.id && state.current.page === p.index;
  if (current) button.setAttribute('aria-current', 'true');
  const text = el('span');
  text.append(el('span', 'name', p.title));
  if (!p.failed) text.append(el('span', 'file', p.file));
  button.title = p.failed ? `${p.file}（変換に失敗しました）` : `${p.title}\n${p.file}`;
  button.append(el('span', 'mark'), text, p.failed ? iconSpan('tail caution', ICON_WARN, '変換に失敗しました') : el('span'));
  li.append(button);
  return li;
}

// ---- 表示 ----

async function showPage(sourceId, pageIndex) {
  const seq = ++state.showSeq;
  state.current = { source: sourceId, page: pageIndex };
  showOpened();
  renderList();
  let data;
  try {
    data = await api('page', { source: sourceId, page: pageIndex });
  } catch (e) {
    if (seq === state.showSeq) toast(e.message);
    return;
  }
  if (seq !== state.showSeq) return;
  document.title = `${data.title} - XML Viewer`;

  if (data.error) {
    const e = data.error;
    const position = e.line ? `（${e.line} 行目${e.column ? `、${e.column} 文字目` : ''}）` : '';
    showMessage('このページは表示できません', ['変換に失敗しました。ほかのページは表示できます。'], [
      ['ファイル', e.file ? e.file + position : ''],
      ['理由', e.reason],
    ]);
    return;
  }

  const url = URL.createObjectURL(new Blob([data.html], { type: 'text/html;charset=utf-8' }));
  const frame = document.createElement('iframe');
  // スクリプトを許さない（allow-scripts を付けない）。allow-same-origin は、親から中身を扱うため。
  frame.setAttribute('sandbox', 'allow-same-origin allow-modals');
  frame.title = data.title;
  frame.addEventListener('load', () => wireDocument(frame, sourceId, pageIndex));
  frame.src = url;
  $('message').hidden = true;
  $('frame-host').replaceChildren(frame);
  if (state.blobUrl) URL.revokeObjectURL(state.blobUrl);
  state.blobUrl = url;
  $('btn-print').disabled = false;
}

/** 表示した文書に、折り返しの指定と、リンク・キー操作の処理を付ける。 */
function wireDocument(frame, sourceId, pageIndex) {
  let doc;
  try {
    doc = frame.contentDocument;
  } catch {
    return;
  }
  if (!doc || !doc.documentElement) return;
  // e-Gov 公式の表示機能と同じく、IE と同じ折り返しにする。
  const style = doc.createElement('style');
  style.textContent = 'pre { white-space: pre-wrap !important; }';
  (doc.head || doc.documentElement).append(style);
  doc.addEventListener('click', (ev) => onDocumentClick(ev, doc, sourceId, pageIndex), true);
  doc.addEventListener('auxclick', (ev) => { if (closestLink(ev.target)) ev.preventDefault(); }, true);
  doc.addEventListener('submit', (ev) => ev.preventDefault(), true);
  doc.addEventListener('keydown', onKeyDown, true);
}

function closestLink(target) {
  return target && typeof target.closest === 'function' ? target.closest('a[href], area[href]') : null;
}

async function onDocumentClick(ev, doc, sourceId, pageIndex) {
  const link = closestLink(ev.target);
  if (!link) return;
  ev.preventDefault();
  const href = (link.getAttribute('href') || '').trim();
  if (href.startsWith('#')) {
    const id = decodeURIComponent(href.slice(1));
    const target = id ? doc.getElementById(id) || doc.getElementsByName(id)[0] : doc.body;
    if (target) target.scrollIntoView();
    return;
  }
  try {
    const r = await api('link', { source: sourceId, page: pageIndex, href });
    if (r.kind === 'page') showPage(sourceId, r.page);
    else if (r.kind === 'none') toast(`このリンクの行き先は開けません（${href}）`);
  } catch (e) {
    toast(e.message);
  }
}

function printPage() {
  const frame = $('frame-host').querySelector('iframe');
  if (!frame || $('btn-print').disabled) return;
  try {
    frame.contentWindow.focus();
    frame.contentWindow.print();
  } catch (e) {
    toast('印刷できませんでした: ' + e.message);
  }
}

// ---- キー操作 ----

/** 画面と文書の両方に付ける。ウィンドウ全体に効く標準の動き（読み直し・戻る・印刷）を止める。 */
function onKeyDown(ev) {
  const ctrl = ev.ctrlKey || ev.metaKey;
  const key = ev.key.length === 1 ? ev.key.toLowerCase() : ev.key;
  const blocked = key === 'F5' || key === 'BrowserRefresh' || key === 'BrowserBack' || key === 'BrowserForward'
    || (ctrl && (key === 'r' || key === 's' || key === 'u'))
    || (ev.altKey && (key === 'ArrowLeft' || key === 'ArrowRight'));
  if (blocked) {
    ev.preventDefault();
    return;
  }
  if (ctrl && key === 'p') {
    ev.preventDefault();
    printPage();
  } else if (ctrl && key === 'o') {
    ev.preventDefault();
    pickAndOpen();
  } else if (key === 'Escape' && !$('settings').hidden) {
    ev.preventDefault();
    closeSettings();
  }
}

// ---- 履歴 ----

async function renderRecent() {
  let items = [];
  try {
    items = (await api('recent')).items;
  } catch (e) {
    toast(e.message);
  }
  const rows = items.map((item) => {
    const li = el('li');
    const open = el('button', 'open');
    open.type = 'button';
    open.dataset.path = item.path;
    open.title = item.path;
    open.append(el('strong', null, baseName(item.path)), el('span', null, dirName(item.path)));
    const time = el('time', null, formatTime(item.openedAt));
    time.dateTime = new Date(item.openedAt * 1000).toISOString();
    const forget = el('button', 'icon-btn');
    forget.type = 'button';
    forget.dataset.forget = item.path;
    forget.setAttribute('aria-label', `${baseName(item.path)} を履歴から消す`);
    forget.title = '履歴から消す';
    forget.innerHTML = ICON_CLOSE;
    li.append(open, time, forget);
    return li;
  });
  $('recent').replaceChildren(...rows);
  $('recent').hidden = !items.length;
  $('recent-none').hidden = items.length > 0;
  $('btn-clear').hidden = !items.length;
}

// ---- 設定 ----

function applyIntegration(items) {
  let stale = false;
  for (const item of items) {
    const sw = document.querySelector(`.switch[data-key="${item.key}"]`);
    if (sw) sw.setAttribute('aria-checked', String(item.state !== 'off'));
    if (item.state === 'stale') stale = true;
  }
  $('moved').hidden = !stale;
}

async function openSettings() {
  $('settings').hidden = false;
  $('settings-close').focus();
  try {
    applyIntegration((await api('integration')).items);
  } catch (e) {
    toast(e.message);
  }
}

function closeSettings() {
  $('settings').hidden = true;
  $('btn-settings').focus();
}

async function toggleIntegration(sw) {
  const enabled = sw.getAttribute('aria-checked') !== 'true';
  sw.disabled = true;
  try {
    applyIntegration((await api('integrationSet', { key: sw.dataset.key, enabled })).items);
  } catch (e) {
    toast(e.message);
  } finally {
    sw.disabled = false;
  }
}

async function repairIntegration() {
  try {
    applyIntegration((await api('integrationRepair')).items);
  } catch (e) {
    toast(e.message);
  }
}

// ---- Rust 側から呼ばれる関数 ----

window.__xmlv = {
  /** ファイルがウィンドウの上に来た・離れた。 */
  drag(over) {
    $('drop').hidden = !over;
  },
  /** ファイルが置かれた。 */
  drop(paths) {
    $('drop').hidden = true;
    if (!$('settings').hidden) closeSettings();
    openPaths(paths);
  },
};

// ---- 起動 ----

function wire() {
  $('btn-open').addEventListener('click', pickAndOpen);
  $('btn-open-2').addEventListener('click', pickAndOpen);
  $('btn-print').addEventListener('click', printPage);
  $('btn-settings').addEventListener('click', openSettings);
  $('settings-close').addEventListener('click', closeSettings);
  $('btn-repair').addEventListener('click', repairIntegration);
  $('toast-close').addEventListener('click', () => { $('toast').hidden = true; });
  document.addEventListener('keydown', onKeyDown, true);

  $('list').addEventListener('click', (ev) => {
    const target = ev.target.closest('button');
    if (!target) return;
    if (target.dataset.close) closeSource(Number(target.dataset.close));
    else if (target.dataset.page !== undefined) showPage(Number(target.dataset.source), Number(target.dataset.page));
    else if (target.dataset.other !== undefined) {
      api('openOther', { source: Number(target.dataset.source), index: Number(target.dataset.other) }).catch((e) => toast(e.message));
    }
  });

  $('recent-block').addEventListener('click', async (ev) => {
    const target = ev.target.closest('button');
    if (!target) return;
    try {
      if (target.dataset.path) await openPaths([target.dataset.path]);
      else if (target.dataset.forget) {
        await api('recentRemove', { path: target.dataset.forget });
        renderRecent();
      } else if (target.id === 'btn-clear') {
        await api('recentClear');
        renderRecent();
      }
    } catch (e) {
      toast(e.message);
    }
  });

  $('settings').addEventListener('click', (ev) => {
    if (ev.target === $('settings')) closeSettings();
    const sw = ev.target.closest('.switch');
    if (sw && !sw.disabled) toggleIntegration(sw);
  });
}

async function init() {
  wire();
  let paths = [];
  try {
    paths = (await api('startup')).paths;
  } catch (e) {
    toast(e.message);
  }
  if (paths.length) await openPaths(paths);
  if (!state.sources.length) showEmpty();
}

init();
