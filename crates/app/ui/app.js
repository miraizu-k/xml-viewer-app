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
  /** 文書を表示しているか。印刷と拡大・縮小は、このときだけ使える。 */
  documentShown: false,
  /** 文書の拡大率（%）。起動時に保存したものを読む。 */
  zoom: 100,
  /** まとまりごとに最後に表示したページ（source id → page）。選び直したときに戻す。 */
  lastPages: new Map(),
};

const $ = (id) => document.getElementById(id);

const ICON_CLOSE = '<svg viewBox="0 0 10 10" aria-hidden="true"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" fill="none" stroke-width="1.3"/></svg>';
const ICON_OPEN = '<svg viewBox="0 0 13 13" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true"><path d="M5 2.5H2.5v8h8V8M7.5 2.5h3v3M10.5 2.5L6 7"/></svg>';
const ICON_WARN = '<svg viewBox="0 0 13 13" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true"><path d="M6.5 1.5l5.5 10h-11z"/><path d="M6.5 5.5v3M6.5 10v.5"/></svg>';

// ---- Rust 側の呼び出し ----

async function api(command, args = {}) {
  let data;
  try {
    const res = await fetch('/api/' + command, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-Xmlv': '1' },
      body: JSON.stringify(args),
    });
    data = await res.json();
  } catch {
    throw new Error('アプリの中の呼び出しに失敗しました');
  }
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
  toastTimer = setTimeout(hideToast, 15000);
}

function hideToast() {
  clearTimeout(toastTimer);
  $('toast').hidden = true;
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
  setDocumentControls(false);
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
  setDocumentControls(false);
}

/** 印刷と拡大・縮小は、文書を表示しているときだけ押せるようにする。 */
function setDocumentControls(shown) {
  state.documentShown = shown;
  $('btn-print').disabled = !shown;
  updateZoomControls();
}

// ---- 開く ----

/** 開く処理の順番待ち。ドロップや履歴のクリックが重なっても、1 つずつ順に開く。 */
let openQueue = Promise.resolve();

function openPaths(paths) {
  const run = openQueue.then(() => openPathsNow(paths));
  openQueue = run.catch(() => {});
  return run;
}

async function openPathsNow(paths) {
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
  let lastPage;
  for (const r of results) {
    if (!r.ok) {
      failures.push(`${r.path}（${r.error.message}）`);
      continue;
    }
    const s = r.source;
    // 新しく開いたものは一覧の上に足す。開いているものは並びを変えない。
    if (!state.sources.some((x) => x.id === s.id)) state.sources = [s, ...state.sources];
    last = s;
    // 開いているまとまりの中の XML を開いたときは、そのページを出す。ほかは最後に見ていたページへ戻す。
    lastPage = r.existing && r.page != null ? r.page : undefined;
    if (s.truncated) notes.push(`${s.name}: ページが多いため、${s.pages.length} 件だけを一覧に出しています。`);
    if (s.skippedEntries) notes.push(`${s.name}: 名前に問題がある ${s.skippedEntries} 件のファイルは展開しませんでした。`);
  }

  if (last) {
    showOpened();
    const page = lastPage !== undefined ? lastPage : pageToShow(last);
    // 表示中のページをもう一度開いたときは、読み直さない。
    if (state.current && state.current.source === last.id && state.current.page === page) renderList();
    else select(last, page);
  } else if (!state.sources.length) {
    showEmpty();
  }
  const messages = [];
  if (failures.length) messages.push('開けませんでした: ' + failures.join('、'));
  messages.push(...notes);
  if (messages.length) toast(messages.join('\n'));
}

/** ファイル選択ダイアログを出しているか。ボタンの連打などで 2 つ目を出さないようにする。 */
let picking = false;

async function pickAndOpen() {
  if (picking) return;
  picking = true;
  let paths;
  try {
    paths = (await api('pick')).paths;
  } catch (e) {
    toast(e.message);
    return;
  } finally {
    picking = false;
  }
  await openPaths(paths);
}

/** まとまりを選んだときに出すページ。最後に見ていたページ、無ければ最初に出すページ。 */
function pageToShow(source) {
  return state.lastPages.get(source.id) ?? source.initialPage ?? null;
}

/** 開いたものを選ぶ。出すページ（省略したら pageToShow）があれば表示する。 */
function select(source, page = pageToShow(source)) {
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
  state.lastPages.delete(id);
  if (state.current && state.current.source === id) {
    clearView();
    if (state.sources.length) {
      select(state.sources[0]);
    } else {
      // 一覧ごと消えるので、閉じるボタンにあったフォーカスを「ファイルを開く」へ移す。
      const hadFocus = $('list').contains(document.activeElement);
      showEmpty();
      if (hadFocus) $('btn-open-2').focus();
    }
  } else {
    renderList();
  }
}

// ---- 一覧 ----

/**
 * 一覧を描き直す。一覧の中にフォーカスがあれば、描き直した後の同じボタンへ移す。
 * 同じボタンが無くなったとき（閉じたとき）は、選んでいるページへ移す。
 */
function renderList() {
  const list = $('list');
  const focused = list.contains(document.activeElement) ? document.activeElement : null;
  const selector = focused ? listButtonSelector(focused) : null;
  list.replaceChildren(...state.sources.map(renderSource));
  revealCurrent();
  if (!focused) return;
  const target = (selector && list.querySelector(selector)) || list.querySelector('.entry[aria-current="true"]')
    || list.querySelector('button');
  if (target) target.focus({ preventScroll: true });
}

/** 選んでいるページが一覧の外に隠れていたら、見える位置までスクロールする。 */
function revealCurrent() {
  const list = $('list');
  const item = list.querySelector('.entry[aria-current="true"]');
  if (!item || list.hidden) return;
  const box = list.getBoundingClientRect();
  const rect = item.getBoundingClientRect();
  if (rect.top < box.top) list.scrollTop -= box.top - rect.top;
  else if (rect.bottom > box.bottom) list.scrollTop += rect.bottom - box.bottom;
}

/** 一覧のボタンを、描き直した後に探すためのセレクター。 */
function listButtonSelector(button) {
  const d = button.dataset || {};
  if (d.close) return `[data-close="${d.close}"]`;
  if (d.page !== undefined) return `[data-source="${d.source}"][data-page="${d.page}"]`;
  if (d.other !== undefined) return `[data-source="${d.source}"][data-other="${d.other}"]`;
  return null;
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

/** ページを表示する。fragment（リンクの「#」の後ろ）があれば、その位置までスクロールする。 */
async function showPage(sourceId, pageIndex, fragment = '') {
  const seq = ++state.showSeq;
  state.current = { source: sourceId, page: pageIndex };
  state.lastPages.set(sourceId, pageIndex);
  showOpened();
  renderList();
  let data;
  try {
    data = await api('page', { source: sourceId, page: pageIndex });
  } catch (e) {
    // 一覧では選んだ状態になっているので、前の文書を出したままにしない。
    if (seq === state.showSeq) {
      const source = state.sources.find((s) => s.id === sourceId);
      document.title = source ? `${source.name} - XML Viewer` : 'XML Viewer';
      showMessage('このページは表示できません', [e.message]);
    }
    return;
  }
  if (seq !== state.showSeq) return;
  document.title = `${data.title} - XML Viewer`;

  if (data.error) {
    const e = data.error;
    const position = e.line ? `（${e.line} 行目${e.column ? `、${e.column} 文字目` : ''}）` : '';
    const source = state.sources.find((s) => s.id === sourceId);
    const others = source && source.pages.some((p, i) => i !== pageIndex && !p.failed);
    showMessage('このページは表示できません', [others ? '変換に失敗しました。ほかのページは表示できます。' : '変換に失敗しました。'], [
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
  // 文書の中のリンクで移ったときは、次の文書にもフォーカスを渡す（キーでのスクロールを続けられるように）。
  const frameFocused = document.activeElement && document.activeElement.tagName === 'IFRAME';
  frame.addEventListener('load', () => {
    wireDocument(frame, sourceId, pageIndex);
    if (fragment && frame.contentDocument) scrollToFragment(frame.contentDocument, fragment);
    if (frameFocused && seq === state.showSeq) frame.contentWindow.focus();
  });
  frame.src = url;
  $('message').hidden = true;
  $('frame-host').replaceChildren(frame);
  if (state.blobUrl) URL.revokeObjectURL(state.blobUrl);
  state.blobUrl = url;
  setDocumentControls(true);
}

/** 表示した文書に、折り返しの指定と拡大率、リンク・キー・ホイールの処理を付ける。 */
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
  applyZoom(doc);
  doc.addEventListener('click', (ev) => onDocumentClick(ev, doc, sourceId, pageIndex), true);
  doc.addEventListener('auxclick', (ev) => { if (closestLink(ev.target)) ev.preventDefault(); }, true);
  doc.addEventListener('submit', (ev) => ev.preventDefault(), true);
  doc.addEventListener('keydown', onKeyDown, true);
  doc.addEventListener('wheel', onDocumentWheel, { capture: true, passive: false });
}

function closestLink(target) {
  return target && typeof target.closest === 'function' ? target.closest('a[href], area[href]') : null;
}

async function onDocumentClick(ev, doc, sourceId, pageIndex) {
  const link = closestLink(ev.target);
  if (!link) return;
  ev.preventDefault();
  // ダブルクリックの 2 回目では開かない（同じファイルやページが 2 回開かないように）。
  if (ev.detail > 1) return;
  const href = (link.getAttribute('href') || '').trim();
  if (!href) return;
  const hash = href.indexOf('#');
  const fragment = hash >= 0 ? href.slice(hash + 1) : '';
  if (hash === 0) {
    scrollToFragment(doc, fragment);
    return;
  }
  // 応答を待つ間に別のページを選んでいたら、リンク先へは移らない。
  const seq = state.showSeq;
  try {
    const r = await api('link', { source: sourceId, page: pageIndex, href });
    if (r.kind === 'page') {
      if (seq !== state.showSeq) return;
      // 同じページへのリンクは、読み直さずにその位置（無ければ先頭）までスクロールする。
      if (r.page === pageIndex) scrollToFragment(doc, fragment);
      else showPage(sourceId, r.page, fragment);
    } else if (r.kind === 'none') {
      toast(`このリンクの行き先は開けません（${href}）`);
    }
  } catch (e) {
    toast(`開けませんでした: ${href}（${e.message}）`);
  }
}

/** 文書の中の、名前（id か name）が fragment の要素までスクロールする。fragment が空なら先頭へ。 */
function scrollToFragment(doc, fragment) {
  let id = fragment;
  try {
    id = decodeURIComponent(id);
  } catch {
    // 「%」がそのまま入っているなどで読めなければ、書かれたままの名前で探す。
  }
  const target = id ? doc.getElementById(id) || doc.getElementsByName(id)[0] : doc.body || doc.documentElement;
  if (target) target.scrollIntoView();
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

// ---- 文書の拡大・縮小 ----
//
// 画面（一覧やツールバー）はそのままで、文書だけを拡大・縮小する。
// WebView2 標準の拡大（画面全体に効く）は Rust 側で切ってある。

/** 拡大率の段階（%）。Edge と同じ。 */
const ZOOM_STEPS = [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300];
const ZOOM_STORAGE_KEY = 'xmlv.zoom';
const ZOOM_STYLE_ID = 'xmlv-zoom';

/** 前回の拡大率を読む。読めなければ 100%。 */
function loadZoom() {
  try {
    const value = Number(localStorage.getItem(ZOOM_STORAGE_KEY));
    if (ZOOM_STEPS.includes(value)) return value;
  } catch {
    // 保存先が使えなくても 100% で動く。
  }
  return 100;
}

/** 表示中の文書。無ければ null。 */
function shownDocument() {
  const frame = $('frame-host').querySelector('iframe');
  try {
    const doc = frame && frame.contentDocument;
    return doc && doc.documentElement ? doc : null;
  } catch {
    return null;
  }
}

/** 文書に今の拡大率を当てる。画面に出すときだけに効かせ、印刷は 100% のままにする。 */
function applyZoom(doc) {
  let style = doc.getElementById(ZOOM_STYLE_ID);
  if (!style) {
    style = doc.createElement('style');
    style.id = ZOOM_STYLE_ID;
    (doc.head || doc.documentElement).append(style);
  }
  style.textContent = state.zoom === 100 ? '' : `@media screen { :root { zoom: ${state.zoom / 100} !important; } }`;
}

function setZoom(value) {
  if (!ZOOM_STEPS.includes(value) || value === state.zoom) return;
  state.zoom = value;
  try {
    localStorage.setItem(ZOOM_STORAGE_KEY, String(value));
  } catch {
    // 保存できなくても、このウィンドウでは効く。
  }
  updateZoomControls();
  const doc = shownDocument();
  if (!doc) return;
  // 拡大・縮小の前後で、見ていた位置（全体に対する割合）を保つ。
  const root = doc.scrollingElement || doc.documentElement;
  const ratioY = root.scrollHeight ? root.scrollTop / root.scrollHeight : 0;
  const ratioX = root.scrollWidth ? root.scrollLeft / root.scrollWidth : 0;
  applyZoom(doc);
  root.scrollTop = ratioY * root.scrollHeight;
  root.scrollLeft = ratioX * root.scrollWidth;
}

/** 1 段階拡大（direction = 1）または縮小（-1）する。 */
function zoomStep(direction) {
  const i = ZOOM_STEPS.indexOf(state.zoom);
  setZoom(ZOOM_STEPS[Math.min(Math.max(i + direction, 0), ZOOM_STEPS.length - 1)]);
}

function updateZoomControls() {
  const shown = state.documentShown;
  $('zoom-level').textContent = `${state.zoom}%`;
  $('btn-zoom-out').disabled = !shown || state.zoom === ZOOM_STEPS[0];
  $('btn-zoom-in').disabled = !shown || state.zoom === ZOOM_STEPS[ZOOM_STEPS.length - 1];
  $('btn-zoom-reset').disabled = !shown;
  // 押していたボタンが端（50% や 300%）で押せなくなったら、フォーカスを「100%」へ移す（外れて先頭に戻らないように）。
  const active = document.activeElement;
  if ((active === $('btn-zoom-in') || active === $('btn-zoom-out')) && active.disabled && shown) {
    $('btn-zoom-reset').focus();
  }
}

let wheelDelta = 0;

/** 文書の上での Ctrl + ホイール（タッチパッドのピンチも同じ形で届く）で拡大・縮小する。 */
function onDocumentWheel(ev) {
  if (!ev.ctrlKey) return;
  ev.preventDefault();
  // マウスのホイールは 1 目盛りで 100 前後、ピンチは小さな値が続けて届くので、溜めてから 1 段階動かす。
  wheelDelta += ev.deltaMode === 1 ? ev.deltaY * 33 : ev.deltaY;
  if (Math.abs(wheelDelta) >= 50) {
    zoomStep(wheelDelta < 0 ? 1 : -1);
    wheelDelta = 0;
  }
}

/** Ctrl と一緒に押されたキーが拡大（1）・縮小（-1）・100%（0）のどれか。どれでもなければ null。 */
function zoomKey(ev) {
  // 日本語キーボードの「;+」キー（Semicolon）と、US キーボードの「=+」キー（Equal）の両方で拡大する。
  if (ev.code === 'Equal' || ev.code === 'Semicolon' || ev.code === 'NumpadAdd' || ev.key === '+') return 1;
  if (ev.code === 'Minus' || ev.code === 'NumpadSubtract' || ev.key === '-') return -1;
  if (ev.code === 'Digit0' || ev.code === 'Numpad0') return 0;
  return null;
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
  // 設定を開いている間は、後ろの画面の操作（開く・印刷・拡大）をしない。標準の動きは止めておく。
  const modal = settingsOpen();
  if (key === 'Escape') {
    if (modal) {
      ev.preventDefault();
      closeSettings();
    } else if (!$('toast').hidden) {
      ev.preventDefault();
      hideToast();
    }
    return;
  }
  const zoom = ctrl && !ev.altKey ? zoomKey(ev) : null;
  if (zoom !== null) {
    ev.preventDefault();
    if (modal || !state.documentShown) return;
    if (zoom === 0) setZoom(100);
    else zoomStep(zoom);
    return;
  }
  if (ctrl && key === 'p') {
    ev.preventDefault();
    if (!modal) printPage();
  } else if (ctrl && key === 'o') {
    ev.preventDefault();
    if (!modal) pickAndOpen();
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
  // 履歴の中にフォーカスがあれば、描き直した後も同じ行（消したときは次の行、無ければ「ファイルを開く」）へ移す。
  const focused = $('recent-block').contains(document.activeElement) ? document.activeElement : null;
  const row = focused ? [...$('recent').children].indexOf(focused.closest('li')) : -1;
  $('recent').replaceChildren(...rows);
  $('recent').hidden = !items.length;
  $('recent-none').hidden = items.length > 0;
  $('btn-clear').hidden = !items.length;
  if (focused) {
    const next = row >= 0 && rows.length ? rows[Math.min(row, rows.length - 1)].querySelector('.open') : null;
    (next || $('btn-open-2')).focus({ preventScroll: true });
  }
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

function settingsOpen() {
  return !$('settings').hidden;
}

/** 設定を開いている間は、後ろの画面（ツールバー、一覧、文書）を操作できないようにする。Tab でも移らない。 */
function setBackgroundInert(on) {
  for (const node of document.querySelectorAll('.toolbar, .body')) node.inert = on;
}

async function openSettings() {
  $('settings').hidden = false;
  setBackgroundInert(true);
  $('settings-close').focus();
  // 実際の登録を読み終わるまでは、前回の表示のまま押されないようにする（読めなければ押せないまま）。
  setIntegrationBusy(true);
  try {
    applyIntegration((await api('integration')).items);
    setIntegrationBusy(false);
  } catch (e) {
    toast(e.message);
  }
}

function closeSettings() {
  $('settings').hidden = true;
  setBackgroundInert(false);
  $('btn-settings').focus();
}

/** 押していたスイッチやボタン。押せない間に外れたフォーカスを、押せるようになったら戻す。 */
let integrationFocus = null;

/** 登録を読み書きしている間は、スイッチと「登録し直す」をまとめて押せないようにする（応答の順序が入れ替わらないように）。 */
function setIntegrationBusy(on) {
  const controls = [...document.querySelectorAll('.switch'), $('btn-repair')];
  if (on) integrationFocus = controls.includes(document.activeElement) ? document.activeElement : null;
  for (const control of controls) control.disabled = on;
  if (on || !integrationFocus) return;
  const lost = !document.activeElement || document.activeElement === document.body;
  // 「登録し直す」は、うまくいくと消えるので、最初のスイッチへ移す。
  const target = integrationFocus.closest('[hidden]') ? document.querySelector('.switch') : integrationFocus;
  integrationFocus = null;
  if (lost && settingsOpen()) target.focus();
}

async function toggleIntegration(sw) {
  const enabled = sw.getAttribute('aria-checked') !== 'true';
  await updateIntegration('integrationSet', { key: sw.dataset.key, enabled });
}

async function repairIntegration() {
  await updateIntegration('integrationRepair');
}

async function updateIntegration(command, args) {
  setIntegrationBusy(true);
  try {
    applyIntegration((await api(command, args)).items);
  } catch (e) {
    toast(e.message);
  } finally {
    setIntegrationBusy(false);
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
    if (settingsOpen()) closeSettings();
    openPaths(paths);
  },
};

// ---- 起動 ----

function wire() {
  $('btn-open').addEventListener('click', pickAndOpen);
  $('btn-open-2').addEventListener('click', pickAndOpen);
  $('btn-print').addEventListener('click', printPage);
  $('btn-zoom-out').addEventListener('click', () => zoomStep(-1));
  $('btn-zoom-in').addEventListener('click', () => zoomStep(1));
  $('btn-zoom-reset').addEventListener('click', () => setZoom(100));
  state.zoom = loadZoom();
  updateZoomControls();
  $('btn-settings').addEventListener('click', openSettings);
  $('settings-close').addEventListener('click', closeSettings);
  $('btn-repair').addEventListener('click', repairIntegration);
  $('toast-close').addEventListener('click', hideToast);
  document.addEventListener('keydown', onKeyDown, true);

  // ダブルクリックの 2 回目は無視する（2 回開いたり、描き直した後の隣のボタンを押したりしないように）。
  $('list').addEventListener('click', (ev) => {
    const target = ev.target.closest('button');
    if (!target || ev.detail > 1) return;
    if (target.dataset.close) closeSource(Number(target.dataset.close));
    else if (target.dataset.page !== undefined) showPage(Number(target.dataset.source), Number(target.dataset.page));
    else if (target.dataset.other !== undefined) {
      const name = target.querySelector('.name').textContent;
      api('openOther', { source: Number(target.dataset.source), index: Number(target.dataset.other) })
        .catch((e) => toast(`開けませんでした: ${name}（${e.message}）`));
    }
  });

  $('recent-block').addEventListener('click', async (ev) => {
    const target = ev.target.closest('button');
    if (!target || ev.detail > 1) return;
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

  // 外側（暗くした部分）で押して離したときだけ閉じる。ダイアログの中で押して外で離したときは閉じない。
  let pressedOutside = false;
  $('settings').addEventListener('pointerdown', (ev) => { pressedOutside = ev.target === $('settings'); });
  $('settings').addEventListener('click', (ev) => {
    if (ev.target === $('settings')) {
      if (pressedOutside) closeSettings();
      return;
    }
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
  if (!state.sources.length && $('empty').hidden) showEmpty();
}

init();
