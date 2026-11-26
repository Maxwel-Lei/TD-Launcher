/* TD 工程启动器 · 前端
   界面沿用 demo/index.html；数据来自 Rust 后端（src-tauri/src/lib.rs）：
     scan_projects   扫描根目录下的工程，加上登记过的外部工程
     inspect_import  导入前看一眼选的文件夹里有哪些工程（只读）
     set_folder_icon 给工程文件夹换上 / 撤回图标
     read_image / save_custom_logo / clear_custom_logo / custom_logo  自定义标志
     list_user_themes / open_themes_dir  自己做的主题
     list_templates / open_templates_dir / save_as_template  自己做的工程模板
     create_project  按模板建文件夹
     save_meta       写回 project.json
     open_project    用系统默认程序（TD）打开 .toe
     reveal          在 Finder / 资源管理器中显示
     delete_project  移到回收站
     move_project    移到另一个分类文件夹 */
const { invoke, convertFileSrc } = window.__TAURI__.core;
const dialog = window.__TAURI__.dialog;

// 自绘标题栏按钮（系统标题栏已关掉）
(() => {
  const w = window.__TAURI__.window && window.__TAURI__.window.getCurrentWindow();
  if (!w) return;
  document.getElementById('wMin').onclick = () => w.minimize();
  document.getElementById('wMax').onclick = () => w.toggleMaximize();
  document.getElementById('wClose').onclick = () => w.close();
})();

const STATUSES = ['进行中', '已完成', '搁置'];
// 早期版本把「已完成」叫「已交付」：读到旧写法时当成已完成，下次保存就会写成新的
const STATUS_ALIAS = { '已交付': '已完成' };
const ICON = { '全部': 's0', '进行中': 's1', '已完成': 's2', '搁置': 's3', '未标注': 's4' };
const ui = { status: '全部', tag: '', group: null, q: '', openId: null, scanning: false, error: '' };
const topGroup = p => p.group ? p.group.split('/')[0] : '';
let root = localStorage.getItem('root') || '';
let projects = [];
const IS_WIN = /Windows/i.test(navigator.userAgent);

// 设置：只记在这台电脑上
const flag = (k, d) => { const v = localStorage.getItem(k); return v === null ? d : v === '1'; };
const cfg = { folderIcon: flag('folderIcon', false), datePrefix: flag('datePrefix', true), autoOpen: flag('autoOpen', true), autoRescan: flag('autoRescan', true), autoWm: flag('autoWm', true), groupByFolder: flag('groupByFolder', true) };
function setCfg(k, v) { cfg[k] = v; try { localStorage.setItem(k, v ? '1' : '0'); } catch (e) {} }

/* ================= 主题 =================
   一个主题 = 一个 JSON 文件，里面有浅色（light）和深色（dark）两套颜色，格式见 docs/theme-spec.md。
   自带的主题在 src/themes/（index.json 里登记），自己做的放在"主题文件夹"里，由后端读出来。
   应用主题 = 把颜色写成 CSS 变量；写不全的可选颜色按必填的 10 个算出来。 */
const THEME_CORE = ['bg0', 'bg1', 'bg2', 'line', 'line2', 'ink', 'ink2', 'mute', 'faint', 'onInk'];
const THEME_VARS = { bg0: '--bg0', bg1: '--bg1', bg2: '--bg2', line: '--line', line2: '--line2', ink: '--ink', ink2: '--ink2', mute: '--mute', faint: '--faint',
  onInk: '--on-ink', onInkMute: '--on-ink-mute', inkHover: '--ink-hover', inkLine: '--ink-line', hover: '--hover', thumb: '--thumb',
  scroll: '--scroll', scrollHover: '--scroll-h', skel1: '--skel1', skel2: '--skel2', lift: '--lift', scrim: '--scrim' };
const THEME_BASE = new URL('themes/', document.currentScript.src).href;
const isHex = c => typeof c === 'string' && /^#[0-9a-f]{6}$/i.test(c);
const isColor = c => isHex(c) || (typeof c === 'string' && /^rgba?\([\d\s.,%]{5,40}\)$/i.test(c));
const rgbOf = c => [1, 3, 5].map(i => parseInt(c.slice(i, i + 2), 16));
const mixHex = (a, b, t) => { const x = rgbOf(a), y = rgbOf(b); return '#' + x.map((v, i) => Math.round(v + (y[i] - v) * t).toString(16).padStart(2, '0')).join(''); };
const alphaOf = (c, a) => `rgba(${rgbOf(c).join(',')},${a})`;
// 把一套颜色补全：可选的没写，就按必填的算
function fullTokens(m, dark) {
  const t = Object.assign({}, m), d = (k, v) => { if (!isColor(t[k])) t[k] = v; };
  d('onInkMute', mixHex(t.onInk, t.ink, 0.3));
  d('inkHover', mixHex(t.ink, t.onInk, 0.14));
  d('inkLine', mixHex(t.ink, t.onInk, 0.2));
  d('hover', mixHex(t.bg1, t.ink, 0.05));
  d('thumb', dark ? mixHex(t.bg0, '#000000', 0.5) : '#161616');
  d('scroll', t.line2); d('scrollHover', t.faint);
  d('skel1', mixHex(t.bg2, t.ink, 0.07)); d('skel2', mixHex(t.bg2, t.ink, 0.04));
  d('lift', dark ? 'rgba(0,0,0,.5)' : alphaOf(t.ink, 0.08));
  d('scrim', dark ? 'rgba(0,0,0,.6)' : alphaOf(t.ink, 0.32));
  return t;
}
// 检查一个主题文件的内容；不合格就返回原因（中文，直接给用户看）
function checkTheme(d) {
  if (!d || typeof d !== 'object' || Array.isArray(d)) return '内容不是一个主题';
  if (d.format !== 1) return 'format 应该是 1';
  if (typeof d.id !== 'string' || !/^[a-z0-9][a-z0-9-]{1,31}$/.test(d.id)) return 'id 要用小写字母、数字、连字符，2 到 32 个字符';
  if (typeof d.name !== 'string' || !d.name.trim()) return '缺少 name';
  for (const mode of ['light', 'dark']) {
    if (!d[mode] || typeof d[mode] !== 'object') return '缺少 ' + mode;
    for (const k of THEME_CORE) {
      if (d[mode][k] === undefined) return `缺少 ${mode}.${k}`;
      if (!isHex(d[mode][k])) return `${mode}.${k} 颜色写法不对，要写成 #RRGGBB`;
    }
    for (const k of Object.keys(THEME_VARS)) if (d[mode][k] !== undefined && !isColor(d[mode][k])) return `${mode}.${k} 颜色写法不对`;
  }
  const lg = d.logo, names = lg === undefined ? [] : typeof lg === 'string' ? [lg] : lg && typeof lg === 'object' ? [lg.light, lg.dark] : [null];
  if (names.some(n => typeof n !== 'string' || !/^[^\\/:*?"<>|]+\.(svg|png)$/i.test(n))) return 'logo 只写同一个文件夹里的文件名（.svg 或 .png）';
  return '';
}
const DEFAULT_THEME = { id: 'default', name: '默认', description: '黑白灰', builtin: true, logo: null,
  light: { bg0: '#E9E9E5', bg1: '#F4F4F1', bg2: '#FFFFFF', line: '#D9D9D3', line2: '#C9C9C3', ink: '#111111', ink2: '#45453F', mute: '#6B6B65', faint: '#8E8E88', onInk: '#FFFFFF' },
  dark: { bg0: '#0E0E0D', bg1: '#171716', bg2: '#20201E', line: '#2C2C29', line2: '#3D3D39', ink: '#F2F2EE', ink2: '#C4C4BD', mute: '#96968F', faint: '#73736D', onInk: '#111111' } };
let themes = [DEFAULT_THEME], themeErrors = [];
let theme = localStorage.getItem('theme') || 'default';
// 外观：light 浅色 / dark 深色 / auto 跟随系统。上一版只有"玉"这个深色主题，没存过外观的沿用它
let themeMode = localStorage.getItem('themeMode') || (theme === 'yu' ? 'dark' : 'light');
const sysDark = matchMedia('(prefers-color-scheme: dark)');
const isDark = () => themeMode === 'dark' || (themeMode === 'auto' && sysDark.matches);
const curTheme = () => themes.find(t => t.id === theme) || themes[0];
function setVars(vars, dark) {
  const st = document.documentElement.style;
  for (const k in vars) st.setProperty(k, vars[k]);
  st.colorScheme = dark ? 'dark' : 'light';
  document.documentElement.dataset.mode = dark ? 'dark' : 'light';
}
function applyTheme() {
  const th = curTheme(), dark = isDark(), tk = fullTokens(dark ? th.dark : th.light, dark), vars = {};
  for (const k in THEME_VARS) vars[THEME_VARS[k]] = tk[k];
  setVars(vars, dark);
  document.documentElement.dataset.theme = th.id;
  // 记下这次的颜色，下次启动先用它，免得主题文件读出来之前闪一下默认配色
  try { localStorage.setItem('themeCache', JSON.stringify({ vars, dark })); } catch (e) {}
}
try { const c = JSON.parse(localStorage.getItem('themeCache') || 'null'); if (c && c.vars) setVars(c.vars, !!c.dark); } catch (e) {}
sysDark.addEventListener('change', () => { if (themeMode === 'auto') { applyTheme(); showLogo(customLogo); } });
function themeLogo(th) {
  if (!th.logo) return '';
  return typeof th.logo === 'string' ? th.logo : (isDark() ? th.logo.dark : th.logo.light) || '';
}
// 读主题：自带的从 themes/index.json，自己做的从后端
async function loadThemes() {
  const list = [], errs = [], seen = new Set();
  const add = (d, file, urlOf, builtin) => {
    const why = checkTheme(d) || (seen.has(d.id) ? 'id 和已有主题重复' : '');
    if (why) { errs.push(`${file}：${why}`); return; }
    seen.add(d.id);
    const lg = d.logo === undefined ? null : typeof d.logo === 'string' ? urlOf(d.logo) : { light: urlOf(d.logo.light), dark: urlOf(d.logo.dark) };
    list.push({ id: d.id, name: d.name.trim().slice(0, 16), description: String(d.description || '').slice(0, 24), light: d.light, dark: d.dark, logo: lg, builtin });
  };
  try {
    const files = await (await fetch(THEME_BASE + 'index.json')).json();
    for (const f of files) { try { add(await (await fetch(THEME_BASE + f)).json(), f, n => THEME_BASE + n, true); } catch (e) { errs.push(`${f}：读不出来`); } }
  } catch (e) { errs.push('自带主题读不出来'); }
  try {
    for (const u of await invoke('list_user_themes') || []) {
      if (u.error) { errs.push(`${u.file}：${u.error}`); continue; }
      add(u.data, u.file, n => convertFileSrc(u.dir + '/' + n), false);
    }
  } catch (e) {}
  themes = list.length ? list : [DEFAULT_THEME]; themeErrors = errs;
  applyTheme(); showLogo(customLogo);
  if ($('#set').classList.contains('on')) renderThemes();
}

// 导入的工程：不在根目录里，只登记它们的位置，文件留在原处（企划 §3）
// 每条记录 { path, group }：group 是它原来所在的分类文件夹，卡片墙和侧栏按它分组
const normPath = p => String(p).replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();
const parentName = p => { const a = String(p).replace(/[\\/]+$/, '').split(/[\\/]/); return a.length > 1 ? a[a.length - 2] : ''; };
let imports = [];
try {
  // 早期版本只存了位置（字符串）：按它上一级文件夹的名字补上分组
  imports = JSON.parse(localStorage.getItem('imports') || '[]')
    .map(x => (typeof x === 'string' ? { path: x, group: parentName(x) } : x))
    .filter(x => x && typeof x.path === 'string');
} catch (e) { imports = []; }
function saveImports() { try { localStorage.setItem('imports', JSON.stringify(imports)); } catch (e) {} }
function dropImport(path) { imports = imports.filter(x => x.path !== path); saveImports(); }

const $ = s => document.querySelector(s);
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const today = () => { const d = new Date(), p = n => String(n).padStart(2, '0'); return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`; };
const fmtTime = s => { if (!s) return ''; const d = new Date(s * 1000), p = n => String(n).padStart(2, '0'); return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`; };
const shortPath = p => p.replace(/\\/g, '/');

/* ================= 数据 ================= */
function normalize(raw) {
  const m = raw.meta || {};
  const fromFolder = (raw.folder.match(/^(\d{4}-\d{2}-\d{2})/) || [])[1];
  return {
    id: raw.path,
    path: raw.path,
    meta: m,
    folder: raw.folder,
    group: raw.group || '',
    toe: raw.toe,
    thumb: raw.thumb,
    thumbMtime: raw.thumbMtime,
    modified: raw.modified,
    hasMeta: raw.hasMeta,
    // 有 project.json 但里面只记了预览图之类的，还不算标注过
    annotated: raw.hasMeta && !!(m.name || m.status || (Array.isArray(m.tags) && m.tags.length) || m.created),
    thumbAuto: !!raw.thumbAuto,
    thumbPref: typeof m.thumbFile === 'string' ? m.thumbFile : '',
    confidence: raw.confidence || 'maybe',
    external: !!raw.external,
    folderIcon: !!raw.folderIcon,
    toeBases: raw.toeBases || [],
    mainToe: m.mainToe || '',
    name: m.name || raw.name,
    tags: Array.isArray(m.tags) ? m.tags : [],
    status: STATUSES.includes(STATUS_ALIAS[m.status] || m.status) ? (STATUS_ALIAS[m.status] || m.status) : '未标注',
    client: m.client || '',
    note: m.note || '',
    created: m.created || fromFolder || (raw.earliest ? fmtTime(raw.earliest).slice(0, 10) : '') || (raw.modified ? fmtTime(raw.modified).slice(0, 10) : '')
  };
}
const thumbSrc = p => p.thumb ? convertFileSrc(p.thumb) + '?v=' + p.thumbMtime : null;

// 补标注时没勾的文件夹：只记在本机（按根目录分开），不往工程文件夹里写东西
let ignored = new Set();
const ignKey = () => 'ignored:' + root;
function loadIgnored() { try { ignored = new Set(JSON.parse(localStorage.getItem(ignKey()) || '[]')); } catch (e) { ignored = new Set(); } }
function saveIgnored() { try { localStorage.setItem(ignKey(), JSON.stringify([...ignored])); } catch (e) {} }

let scanSeq = 0;
async function scan({ quiet = false } = {}) {
  if (!root) { projects = []; render(); return; }
  const seq = ++scanSeq;
  const slow = quiet ? null : setTimeout(() => { ui.scanning = true; renderView(); }, 180);
  try {
    // 导入之后换过根目录、现在已经在根目录里面的：不用再登记，直接归根目录管
    const r = normPath(root) + '/', kept = imports.filter(x => !normPath(x.path).startsWith(r));
    if (kept.length !== imports.length) { imports = kept; saveImports(); }
    const list = await invoke('scan_projects', { root, extra: imports.map(x => x.path) });
    if (seq !== scanSeq) return;
    loadIgnored();
    // 早期版本会把一个工程里更深的文件夹也单独导入；现在只认直属的文件夹，套在别的工程里面的就不再登记
    const all = list.map(p => normPath(p.path)), inner = new Set(list.filter(p => p.external && all.some(q => normPath(p.path).startsWith(q + '/'))).map(p => p.path));
    if (inner.size) { imports = imports.filter(x => !inner.has(x.path)); saveImports(); for (let i = list.length - 1; i >= 0; i--) if (inner.has(list[i].path)) list.splice(i, 1); }
    // 导入的工程按它原来所在的文件夹分组
    for (const p of list) if (p.external) { const rec = imports.find(x => x.path === p.path); p.group = (rec && rec.group) || '外部导入'; }
    ui.live = new Set(list.map(p => p.path));
    ui.missingN = imports.filter(x => !ui.live.has(x.path)).length;
    ui.ignoredN = list.filter(p => ignored.has(p.path)).length;
    projects = list.filter(p => !ignored.has(p.path)).map(normalize);
    ui.error = '';
  } catch (e) {
    if (seq !== scanSeq) return;
    projects = []; ui.error = String(e);
  } finally {
    clearTimeout(slow); ui.scanning = false;
  }
  render();
  if (ui.openId) { const p = cur(); if (p) refreshDetailHead(p); else closeAll(); }
  if (cfg.folderIcon) syncIcons(true, true);
  fillTypes();
  wmAuto();
}

/* ================= 文件夹图标 ================= */
// 只给已标注（有 project.json）的工程换；换不成的（比如文件夹原来就有自己的图标设置）这次运行里不再重试
const iconFailed = new Set();
let iconBusy = false;
async function syncIcons(on, quiet, redo) {
  if (!IS_WIN || iconBusy) return;
  // redo：标志换了，已经有图标的文件夹也重写一遍
  const list = on ? projects.filter(p => p.hasMeta && (redo || !p.folderIcon) && !iconFailed.has(p.path)) : projects.filter(p => p.folderIcon);
  if (!list.length) { if (!quiet) toast(on ? '所有已标注的工程文件夹都已经换好图标了' : '没有需要撤回的图标'); return; }
  iconBusy = true;
  let ok = 0; const bad = [];
  for (const p of list) {
    try { await invoke('set_folder_icon', { path: p.path, on }); p.folderIcon = on; ok++; }
    catch (e) { iconFailed.add(p.path); bad.push(`「${p.name}」${e}`); }
  }
  iconBusy = false;
  if (!quiet || bad.length) toast((on ? `已给 ${ok} 个工程文件夹换上图标` : `已撤回 ${ok} 个文件夹的图标`) + (bad.length ? `；${bad.length} 个没成功：${bad.slice(0, 3).join('；')}` : '') + (ok && !quiet ? '。资源管理器里没变的话按 F5 刷新' : ''));
}

let saveTimers = {};
function saveMeta(p, delay = 0) {
  clearTimeout(saveTimers[p.id]);
  saveTimers[p.id] = setTimeout(async () => {
    try {
      const meta = { name: p.name, tags: p.tags, status: p.status === '未标注' ? '' : p.status, client: p.client, note: p.note, created: p.created };
      if (p.mainToe) meta.mainToe = p.mainToe;
      await invoke('save_meta', { path: p.path, meta });
      p.hasMeta = true; saved();
    } catch (e) { toast('保存失败：' + e); }
  }, delay);
}

async function pickRoot() {
  const dir = await dialog.open({ directory: true, multiple: false, title: '选择工程根目录', defaultPath: root || undefined });
  if (!dir) return;
  root = Array.isArray(dir) ? dir[0] : dir;
  localStorage.setItem('root', root);
  syncRoot(); ui.status = '全部'; ui.tag = ''; ui.group = null; closeAll(); scan();
}
function syncRoot() {
  $('#rootPath').textContent = root ? shortPath(root) : '未选择';
  $('#rootPath').title = root;
  $('#npRoot').textContent = root ? shortPath(root) : '未选择';
  $('#sRoot').textContent = root ? shortPath(root) : '未选择';
}

/* ================= 渲染 ================= */
function renderSide() {
  const order = ['全部', ...STATUSES];
  if (projects.some(p => p.status === '未标注')) order.push('未标注');
  $('#stList').innerHTML = order.map(s => {
    const n = s === '全部' ? projects.length : projects.filter(p => p.status === s).length;
    return `<button class="st-row cut" type="button" data-st="${s}" aria-pressed="${ui.status === s}"><i class="ico ${ICON[s]}"></i>${s}<span class="n">${n}</span></button>`;
  }).join('');
  const tags = [...new Set(projects.flatMap(p => p.tags))];
  $('#tagList').innerHTML = tags.length ? tags.map(t => `<button class="tag" type="button" data-tag="${esc(t)}" aria-pressed="${ui.tag === t}">${esc(t)}</button>`).join('')
    : '<span style="font-size:12px;color:var(--faint);padding:0 6px">在工程详情里添加标签</span>';
  $('#ignRow').innerHTML = (ui.ignoredN ? `<div class="ign">已忽略 ${ui.ignoredN} 个文件夹 · <button type="button" data-unignore>全部恢复</button></div>` : '') +
    (ui.missingN ? `<div class="ign">有 ${ui.missingN} 个导入的工程现在读不到（被移走或硬盘没插） · <button type="button" data-unmissing>不再登记</button></div>` : '');
  // 文件夹：按根目录下第一层分类文件夹分组；只有一组时不显示
  const groups = [...new Set(projects.map(topGroup))].sort((a, b) => a.localeCompare(b, 'zh'));
  $('#grpSec').hidden = groups.length < 2;
  $('#grpList').innerHTML = groups.map(g => {
    const n = projects.filter(p => topGroup(p) === g).length;
    return `<button class="st-row cut" type="button" data-grp="${esc(g)}" aria-pressed="${ui.group === g}"><span class="gname">${esc(g || '（根目录）')}</span><span class="n">${n}</span></button>`;
  }).join('');
  $('#clear').hidden = !(ui.status !== '全部' || ui.tag || ui.q || ui.group !== null);
}

function cardHTML(p) {
  const th = thumbSrc(p);
  return `<div class="card-wrap${ui.sel.has(p.id) ? ' sel' : ''}" data-id="${esc(p.id)}"><button class="pick" type="button" data-pick="${esc(p.id)}" aria-pressed="${ui.sel.has(p.id)}" aria-label="选择 ${esc(p.name)}"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3.2" stroke-linecap="square"><path d="M4 12.5l5 5L20 6.5"/></svg></button><button class="card cut" type="button" data-open="${esc(p.id)}">
    <div class="thumb${th ? '' : ' empty'}">${th ? `<img src="${th}" alt="" loading="lazy">` : `<div class="hint"><b>还没有预览图</b>${p.toe ? '用 TD 打开并保存一次即可生成，或在详情里选一张图' : '这个文件夹里还没有 .toe'}</div>`}</div>
    <div class="info">
      <div class="r1"><span class="name" title="${esc(p.name)}">${esc(p.name)}</span>${p.external ? '<span class="ext" title="文件在工程根目录外面，留在原处">外部</span>' : ''}<span class="date">${esc(p.created)}</span></div>
      <div class="r2"><i class="ico ${ICON[p.status]}"></i><span class="st">${p.status}</span><span class="sep">/</span><span class="tg">${p.tags.map(esc).join(' · ') || '无标签'}</span></div>
    </div></button></div>`;
}

function emptyHTML(title, text, actions) {
  return `<div class="empty-state">
    <span class="label">${root ? 'No projects yet' : 'Welcome'}</span>
    <h1>${title}</h1><p>${text}</p><div class="acts">${actions}</div></div>`;
}
function renderView() {
  const v = $('#view');
  if (!root) {
    v.innerHTML = emptyHTML('先选择工程根目录', '选一个放 TD 工程的文件夹，里面每个子文件夹就是一个工程。以后新建的工程也会放在这里。',
      '<button class="btn cut" type="button" data-pick-root>选择工程根目录</button>');
  } else if (ui.error) {
    v.innerHTML = emptyHTML('读不到这个文件夹', esc(ui.error), '<button class="btn cut" type="button" data-pick-root>换一个文件夹</button><button class="btn ghost cut cut-line" type="button" data-rescan>重试</button>');
  } else if (ui.scanning) {
    v.innerHTML = `<div class="scan cut"><span class="blink"></span><span class="label">Scanning</span><span>正在扫描 <span class="mono">${esc(shortPath(root))}</span></span></div>
      <div class="grid">${[0, 1, 2, 3, 4, 5].map(i => `<div class="card cut skel"><div class="thumb empty"></div><div class="info" style="gap:10px;padding-bottom:18px"><div class="bar1"></div><div class="bar2"></div></div></div>`).join('')}</div>`;
    return;
  } else if (!projects.length) {
    v.innerHTML = emptyHTML('这里还没有工程', '新建一个工程，或者把工程根目录换成你已有 TD 工程所在的文件夹。',
      '<button class="btn cut" type="button" data-new>新建第一个工程</button><button class="btn ghost cut cut-line" type="button" data-import>导入已有工程</button><button class="btn ghost cut cut-line" type="button" data-pick-root>换一个根目录</button>');
  } else {
    const q = ui.q.trim().toLowerCase();
    let list = projects.filter(p => (ui.status === '全部' || p.status === ui.status) && (!ui.tag || p.tags.includes(ui.tag)) &&
      (ui.group === null || topGroup(p) === ui.group) &&
      (!q || p.name.toLowerCase().includes(q) || p.folder.toLowerCase().includes(q) || p.group.toLowerCase().includes(q)));
    // 按分类文件夹分组：根目录下第一层文件夹各占一段，根目录直接放的排最前，外部导入的排最后；组内仍是最近修改在前
    let secs = null;
    if (cfg.groupByFolder && ui.group === null) {
      const rank = g => (g === '' ? 0 : g === '外部导入' ? 2 : 1);
      const names = [...new Set(list.map(topGroup))].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b, 'zh'));
      if (names.length > 1) { secs = names.map(g => [g, list.filter(p => topGroup(p) === g)]); list = secs.flatMap(s => s[1]); }
    }
    ui.visible = list.map(p => p.id);
    const un = projects.filter(p => !p.annotated).length;
    const annbar = un && (ui.status === '全部' || ui.status === '未标注') ? `<div class="annbar cut"><i class="ico s4"></i><span>有 <b>${un}</b> 个工程还没有 project.json，可以一次补上标注</span><button class="btn text" type="button" data-ann>识别并补标注</button></div>` : '';
    let head = ui.status === '全部' ? '全部工程' : ui.status; if (ui.tag) head += ' · ' + ui.tag; if (ui.group !== null) head += ' · ' + (ui.group || '根目录');
    v.innerHTML = `<div class="main-h"><div><span class="label">Projects</span><h1>${esc(head)}</h1></div>
      <div class="count">共 <span class="num">${list.length}</span> 个工程 · ${secs ? '按文件夹分组，组内最近修改在前' : '最近修改在前'} · 按住 Ctrl 点卡片可多选</div></div>
      ${annbar}${secs ? secs.map(([g, arr]) => `<section class="grp"><div class="grp-h"><h2>${esc(g || '根目录')}</h2><span class="num">${arr.length}</span><button class="btn text" type="button" data-grp="${esc(g)}">只看这一类</button></div><div class="grid">${arr.map(cardHTML).join('')}</div></section>`).join('') :
        list.length ? `<div class="grid">${list.map(cardHTML).join('')}</div>` :
        `<div class="nomatch"><div style="font-weight:500">没有符合条件的工程</div><button class="btn ghost cut cut-line" type="button" data-clear>清除筛选</button></div>`}`;
    return;
  }
}
function render() { renderSide(); renderView(); syncSel(); }

/* ================= 新建工程 ================= */
const BAD = /[:?*\/\\<>|"]/g;
function folderName() {
  const base = $('#npName').value.trim().replace(/[. ]+$/, '');
  if ($('#npCase') && $('#npCase').checked) return { base, folder: `CASE${String(nextCaseNo()).padStart(2, '0')}_${base}` };
  return { base, folder: ($('#npDate').checked ? today() + '_' : '') + base };
}
// 工程模板：'' 是自带的默认模板，其它是"模板文件夹"里自己做的
let templates = [], tplSel = localStorage.getItem('template') || '';
function renderTpls() {
  if (tplSel && !templates.some(t => t.name === tplSel)) tplSel = '';
  const tile = (id, name, sub, tag) => `<button type="button" class="tpl ${id === tplSel ? 'cut' : 'off'}" data-tpl="${esc(id)}" aria-pressed="${id === tplSel}">${tag ? `<span class="label">${tag}</span>` : ''}${esc(name)}<small>${esc(sub)}</small></button>`;
  $('#npTpls').innerHTML = tile('', '默认模板', '素材 / 组件 / 脚本 / 输出', 'Default') +
    templates.map(t => tile(t.name, t.name, (t.entries.filter(e => e.endsWith('/')).map(e => e.slice(0, -1)).slice(0, 4).join(' / ') || '没有子文件夹') + (t.hasToe ? '' : ' · 用自带的 .toe'), '')).join('');
}
async function loadTemplates() {
  try { templates = await invoke('list_templates') || []; } catch (e) { templates = []; }
  renderTpls(); updateTree();
}
function setTpl(id) { tplSel = id; try { localStorage.setItem('template', id); } catch (e) {} renderTpls(); updateTree(); }
function updateTree() {
  const { base, folder } = folderName(), b = base || '未命名', f = folder || b;
  const tp = templates.find(t => t.name === tplSel);
  if (tp) {
    const rest = tp.entries.slice(0, 14), more = tp.entries.length - rest.length;
    const lines = [`├─ ${esc(b)}.toe`, '├─ project.json   ← 标签、状态、备注', '├─ thumb.jpg      ← 保存时自动生成', ...rest.map(e => '├─ ' + esc(e))];
    if (more > 0) lines.push(`├─ … 还有 ${more} 项`);
    lines[lines.length - 1] = lines[lines.length - 1].replace('├─', '└─');
    $('#npTree').innerHTML = [`<b>${esc(f || b)}/</b>`, ...lines].join('\n');
    return;
  }
  $('#npTree').innerHTML = [`<b>${esc(f || b)}/</b>`, `├─ ${esc(b)}.toe`, '├─ project.json   ← 标签、状态、备注', '├─ thumb.jpg      ← 保存时自动生成',
    '├─ assets/', '│  ├─ images/', '│  ├─ videos/', '│  ├─ audio/', '│  ├─ models/', '│  └─ fonts/', '├─ tox/', '├─ scripts/', '└─ export/'].join('\n');
}
$('#npName').addEventListener('input', e => {
  const raw = e.target.value, clean = raw.replace(BAD, '');
  if (clean !== raw) { const pos = Math.max(0, e.target.selectionStart - (raw.length - clean.length)); e.target.value = clean; e.target.setSelectionRange(pos, pos); $('#npWarn').hidden = false; }
  updateTree();
});
$('#npName').addEventListener('keydown', e => { if (e.key === 'Enter') $('#npCreate').click(); });
$('#npDate').addEventListener('change', e => { setCfg('datePrefix', e.target.checked); updateTree(); });
$('#npCreate').addEventListener('click', async () => {
  if (!root) { toast('先选择工程根目录'); return; }
  const { base, folder } = folderName();
  if (!base) { toast('先给工程起个名字'); $('#npName').focus(); return; }
  const btn = $('#npCreate'); btn.disabled = true;
  try {
    const cat = $('#npCat').value;
    const components = (comps || []).filter(c => compSel.has(c.id)).map(c => ({ name: c.name, src: c.src, load: c.load || [] }));
    if (components.some(c => /mediapipe/i.test(c.name + c.src))) toast('正在复制组件，Mediapipe 比较大，要等几秒…');
    const res = await invoke('create_project', { root: cat ? root + '/' + cat : root, folder, name: base, created: today(), template: tplSel || null, components, case: $('#npCase').checked, order: !$('#npCase').checked && /商单|交付/.test(cat) });
    // 类型和编号记进 project.json：案例工程自动取下一个没用过的编号
    const isCaseNew = $('#npCase').checked, kind = isCaseNew ? 'case' : /商单|交付/.test(cat) ? 'order' : '';
    if (kind) {
      const extra = { type: kind };
      if (isCaseNew) { const no = nextCaseNo(); extra.caseId = 'case' + String(no).padStart(2, '0'); try { localStorage.setItem('caseHigh', String(no)); } catch (e) {} }
      await invoke('save_meta', { path: res.path, meta: extra }).catch(() => {});
    }
    closeAll();
    await scan({ quiet: true });
    if (res.toe && !cfg.autoOpen) {
      toast(`已创建「${base}」`);
    } else if (res.toe) {
      try { await invoke('open_project', { path: res.path }); toast(`已创建「${base}」，正在用 TD 打开`); }
      catch (e) { toast(`已创建「${base}」，但打开失败：${e}`); }
    } else {
      toast(`已创建「${base}」· 还没有模板 .toe，请在 TD 里新建并保存到这个文件夹`);
      invoke('reveal', { path: res.path }).catch(() => {});
    }
  } catch (e) { toast('创建失败：' + e); }
  finally { btn.disabled = false; }
});
function openNew() {
  if (!root) { pickRoot(); return; }
  closeAll(); $('#npWarn').hidden = true; $('#npName').value = '';
  $('#npDate').checked = cfg.datePrefix;
  $('#npCreate').textContent = cfg.autoOpen ? '创建并用 TD 打开' : '创建';
  renderTpls(); updateTree(); syncRoot(); loadTemplates(); renderCats(); loadComps().then(() => { compSel = new Set(comps.filter(c => c.on && !c.missing).map(c => c.id)); renderNpComps(); });
  $('#scrim').classList.add('on'); $('#modal').classList.add('on');
  setTimeout(() => $('#npName').focus(), 60);
}

/* ================= 详情抽屉 ================= */
const cur = () => projects.find(x => x.id === ui.openId);
function refreshDetailHead(p) {
  const th = thumbSrc(p);
  $('#dThumb').innerHTML = th ? `<img src="${th}" alt="${esc(p.name)} 的缩略图">` : '';
  $('#dThumb').style.background = th ? 'var(--thumb)' : 'var(--bg1)';
  $('#dName').textContent = p.name;
  $('#dPath').textContent = shortPath(p.path);
  $('#dPath').title = p.external ? '外部导入：文件在工程根目录外面，留在原处' : p.group ? '分类：' + p.group : '';
  $('#dDel').textContent = p.external ? '从启动器移除' : '删除工程';
  $('#dDel').nextElementSibling.textContent = p.external ? '只是不再显示，文件夹和文件都不动' : '整个文件夹移到回收站，可以恢复';
  $('#dMeta').textContent = `创建于 ${p.created || '未知'} · ${th ? (p.thumbAuto ? '预览图是自动找的：' + baseName(p.thumb) : '预览图更新于 ' + fmtTime(p.thumbMtime)) : p.thumbPref === '-' ? '不显示预览图' : '还没有预览图'}${p.toe ? '' : ' · 没有 .toe'}`;
  $('#dThumbActs').innerHTML = `<button class="btn text" type="button" data-thumb="pick">换预览图…</button>` +
    (p.thumbAuto ? '<button class="btn text" type="button" data-thumb="none">不用这张</button>' : '') +
    (p.thumbPref ? '<button class="btn text" type="button" data-thumb="auto">恢复自动</button>' : '');
  $('#dOpen').disabled = !p.toe;
  const many = p.toeBases.length > 1;
  $('#dMainWrap').hidden = !many;
  if (many) {
    const stem = p.toe ? p.toe.split(/[\\/]/).pop().replace(/\.toe$/i, '') : '';
    const curBase = p.toeBases.includes(p.mainToe) ? p.mainToe
      : p.toeBases.filter(b => stem === b || stem.startsWith(b + '.')).sort((a, b) => b.length - a.length)[0] || '';
    $('#dMain').innerHTML = p.toeBases.map(b => `<button type="button" data-amain="${esc(b)}" aria-pressed="${b === curBase}" title="${esc(b)}.toe">${esc(b)}</button>`).join('');
  }
}
function openDetail(id) {
  const p = projects.find(x => x.id === id); if (!p) return;
  ui.openId = id; closeAll(true); ui.openId = id;
  refreshDetailHead(p); renderCase(p);
  $('#dClient').value = p.client; $('#dNote').value = p.note;
  renderDetailBits(p);
  $('#scrim').classList.add('on'); $('#drawer').classList.add('on');
}
function renderDetailBits(p) {
  $('#dStatus').innerHTML = STATUSES.map(s => `<button type="button" data-dst="${s}" aria-pressed="${p.status === s}"><i class="ico ${ICON[s]}"></i>${s}</button>`).join('');
  $('#dTags').innerHTML = p.tags.map(t => `<span class="chip">${esc(t)}<button type="button" data-rm="${esc(t)}" aria-label="删除标签 ${esc(t)}"><svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="square"><path d="M5 5l14 14M19 5L5 19"/></svg></button></span>`).join('') || '<span style="font-size:13px;color:var(--mute)">还没有标签</span>';
}
function saved() { const s = $('#savedTag'); s.style.opacity = 0; setTimeout(() => s.style.opacity = 1, 180); }
function addTag() {
  const p = cur(), v = $('#dTagIn').value.trim(); $('#dTagIn').value = '';
  if (!p || !v || p.tags.includes(v)) return;
  p.tags.push(v); renderDetailBits(p); render(); saveMeta(p);
}
$('#dTagAdd').addEventListener('click', addTag);
$('#dTagIn').addEventListener('keydown', e => { if (e.key === 'Enter') addTag(); });
$('#dClient').addEventListener('input', e => { const p = cur(); if (p) { p.client = e.target.value; saveMeta(p, 500); } });
$('#dNote').addEventListener('input', e => { const p = cur(); if (p) { p.note = e.target.value; saveMeta(p, 500); } });
$('#dOpen').addEventListener('click', async () => { const p = cur(); if (!p) return; try { await invoke('open_project', { path: p.path }); toast('正在用 TD 打开'); } catch (e) { toast('打开失败：' + e); } });
$('#dReveal').addEventListener('click', () => { const p = cur(); if (p) invoke('reveal', { path: p.path }).catch(e => toast('打不开文件夹：' + e)); });

// 主文件：文件夹里有几个不同的 .toe 时，选哪个用 TD 打开（记进 project.json 的 mainToe）
async function setMainToe(base) {
  const p = cur(); if (!p) return;
  try {
    await invoke('save_meta', { path: p.path, meta: { mainToe: base } });
    p.mainToe = base; p.hasMeta = true; saved();
    await scan({ quiet: true });
  } catch (e) { toast('保存失败：' + e); }
}

/* ================= 确认框（删除、批量操作共用） ================= */
let cfmYes = null;
function confirmBox({ label = 'Confirm', title, html, yes, onYes }) {
  $('#cLabel').textContent = label; $('#cTitle').textContent = title;
  $('#cText').innerHTML = html; $('#cYes').textContent = yes; cfmYes = onYes;
  $('#cfm').classList.add('on');
  setTimeout(() => $('#cNo').focus(), 60);
}
function closeConfirm() { $('#cfm').classList.remove('on'); cfmYes = null; }
$('#cNo').addEventListener('click', closeConfirm);
$('#cYes').addEventListener('click', async () => {
  const f = cfmYes; if (!f) return closeConfirm();
  $('#cYes').disabled = true;
  try { await f(); } finally { $('#cYes').disabled = false; closeConfirm(); }
});

// 预览图：旧工程没有 thumb.jpg 时软件会在文件夹里自动找一张；这里可以自己换、不用、或恢复自动
async function setThumb(kind) {
  const p = cur(); if (!p) return;
  let file = kind === 'none' ? null : '';
  if (kind === 'pick') {
    const f = await dialog.open({ multiple: false, title: '选一张图当预览', defaultPath: p.path, filters: [{ name: '图片', extensions: ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'gif'] }] });
    if (!f) return;
    file = Array.isArray(f) ? f[0] : f;
  }
  try { await invoke('set_thumb', { path: p.path, file }); await scan({ quiet: true }); toast(kind === 'pick' ? '预览图已更换' : kind === 'none' ? '已不显示预览图' : '已恢复自动'); }
  catch (e) { toast('没换成：' + e); }
}

// 把这个工程存成模板：只存文件夹结构和最新的 .toe
$('#dTpl').addEventListener('click', () => {
  const p = cur(); if (!p) return;
  confirmBox({
    label: 'Template', title: '存为模板', yes: '保存',
    html: `把「<b>${esc(p.name)}</b>」的文件夹结构和最新的 .toe 存成模板，以后新建工程时可以选。素材、导出的文件不会带上。<br><input class="inp" id="tplName" type="text" value="${esc(p.name)}" aria-label="模板名" style="margin-top:12px;width:100%">`,
    onYes: async () => {
      const name = ($('#tplName').value || '').trim();
      if (!name) { toast('先给模板起个名字'); return; }
      try { await invoke('save_as_template', { path: p.path, name }); toast(`已存为模板「${name}」，新建工程时可以选`); }
      catch (e) { toast('没存成：' + e); }
    }
  });
});

// 导入的工程：只取消登记，不动文件；换过的文件夹图标一起撤回
async function unimport(list) {
  for (const p of list) {
    if (p.folderIcon) { try { await invoke('set_folder_icon', { path: p.path, on: false }); } catch (e) {} }
    dropImport(p.path);
  }
}
// 删除单个工程：整个文件夹移到回收站
$('#dDel').addEventListener('click', () => {
  const p = cur(); if (!p) return;
  if (p.external) {
    confirmBox({
      label: 'Remove', title: '从启动器移除？', yes: '移除',
      html: `「<b>${esc(p.name)}</b>」是导入的工程。移除后只是不再显示在这里，文件夹和里面的文件都不动，以后可以重新导入。<br><span style="font-size:12px;color:var(--mute)">${esc(p.path)}</span>`,
      onYes: async () => { await unimport([p]); closeAll(); projects = projects.filter(x => x !== p); render(); toast(`已移除「${p.name}」，文件没有动`); scan({ quiet: true }); }
    });
    return;
  }
  confirmBox({
    label: 'Delete', title: '删除工程？', yes: '移到回收站',
    html: `「<b>${esc(p.name)}</b>」的整个文件夹（.toe、素材、导出都在里面）会移到系统回收站。<br>删错了可以从回收站还原。<br><span style="font-size:12px;color:var(--mute)">${esc(p.path)}</span>`,
    onYes: async () => {
      try {
        await invoke('delete_project', { root, path: p.path });
        closeAll(); projects = projects.filter(x => x !== p); render();
        toast(`「${p.name}」已移到回收站`);
        scan({ quiet: true });
      } catch (e) { toast('删除失败：' + e); }
    }
  });
});

/* ================= 多选与批量管理（企划 §6.3） ================= */
ui.sel = new Set(); ui.anchor = null; ui.visible = []; ui.ignoredN = 0; ui.missingN = 0;
const selected = () => projects.filter(p => ui.sel.has(p.id));
function syncSel() {
  for (const id of [...ui.sel]) if (!projects.some(p => p.id === id)) ui.sel.delete(id);
  document.body.classList.toggle('selecting', ui.sel.size > 0);
  document.querySelectorAll('.card-wrap[data-id]').forEach(w => {
    const on = ui.sel.has(w.dataset.id);
    w.classList.toggle('sel', on);
    const b = w.querySelector('.pick'); if (b) b.setAttribute('aria-pressed', on);
  });
  $('#bN').textContent = ui.sel.size;
  $('#bbar').classList.toggle('on', ui.sel.size > 0);
  if (!ui.sel.size) closePop();
}
function toggleSel(id, range) {
  if (range && ui.anchor && ui.visible.includes(ui.anchor) && ui.visible.includes(id)) {
    const a = ui.visible.indexOf(ui.anchor), b = ui.visible.indexOf(id);
    for (const x of ui.visible.slice(Math.min(a, b), Math.max(a, b) + 1)) ui.sel.add(x);
  } else {
    if (ui.sel.has(id)) ui.sel.delete(id); else ui.sel.add(id);
    ui.anchor = id;
  }
  syncSel();
}
function clearSel() { ui.sel.clear(); ui.anchor = null; syncSel(); }
function selectAll() { ui.visible.forEach(id => ui.sel.add(id)); syncSel(); }
// 点卡片：Ctrl 点 = 加选，Shift 点 = 连选；已经在多选时，直接点也是加选
function onCardClick(id, e) {
  if (e.shiftKey && ui.anchor) toggleSel(id, true);
  else if (e.ctrlKey || e.metaKey || ui.sel.size) toggleSel(id, false);
  else openDetail(id);
}

function closePop() {
  $('#bpop').classList.remove('on');
  document.querySelectorAll('#bbar [aria-expanded]').forEach(b => b.setAttribute('aria-expanded', 'false'));
}
function openPop(btn, html) {
  const pop = $('#bpop'), was = btn.getAttribute('aria-expanded') === 'true';
  closePop(); if (was) return;
  pop.innerHTML = html; pop.classList.add('on'); btn.setAttribute('aria-expanded', 'true');
  const r = btn.getBoundingClientRect(), bar = $('#bbar').getBoundingClientRect();
  pop.style.left = Math.max(16, Math.min(innerWidth - pop.offsetWidth - 16, r.left)) + 'px';
  pop.style.bottom = (innerHeight - bar.top + 8) + 'px';
  const inp = pop.querySelector('input'); if (inp) setTimeout(() => inp.focus(), 30);
}
$('#bpop').addEventListener('keydown', e => {
  if (e.key !== 'Enter') return;
  if (e.target.id === 'bTagIn') batchTagAdd(e.target.value);
  if (e.target.id === 'bDestIn' && e.target.value.trim()) askBatchMove(e.target.value.trim());
});

// 可以移进去的分类文件夹：根目录 + 现有工程所在的各级分类；工程文件夹本身不算
function moveTargets() {
  const inRoot = projects.filter(p => !p.external);
  const projRel = new Set(inRoot.map(p => p.group ? p.group + '/' + p.folder : p.folder));
  const out = new Set(['']);
  for (const p of inRoot) {
    const parts = p.group ? p.group.split('/') : [];
    for (let i = 1; i <= parts.length; i++) {
      const g = parts.slice(0, i).join('/');
      if ([...projRel].some(r => g === r || g.startsWith(r + '/'))) break;
      out.add(g);
    }
  }
  return [...out].sort((a, b) => a.localeCompare(b, 'zh'));
}

function bact(kind, btn) {
  const sel = selected(); if (!sel.length && kind !== 'all') return;
  if (kind === 'status') {
    openPop(btn, `<div class="ph">把 ${sel.length} 个工程改为</div>` +
      STATUSES.map(s => `<button class="st-row cut" type="button" data-bst="${s}"><i class="ico ${ICON[s]}"></i>${s}</button>`).join(''));
  } else if (kind === 'tagadd') {
    const tags = [...new Set(projects.flatMap(p => p.tags))];
    openPop(btn, `<div class="ph">给 ${sel.length} 个工程加标签</div>
      <div class="addrow"><input class="inp" id="bTagIn" type="text" placeholder="输入标签，回车添加"><button class="btn ghost cut-line" type="button" data-badd="" style="height:40px;--k:0px">添加</button></div>
      ${tags.length ? `<div class="ph">或点一个已有标签</div><div class="tags">${tags.map(t => `<button class="tag" type="button" data-badd="${esc(t)}">${esc(t)}</button>`).join('')}</div>` : ''}`);
  } else if (kind === 'tagrm') {
    const cnt = {}; sel.forEach(p => p.tags.forEach(t => { cnt[t] = (cnt[t] || 0) + 1; }));
    const ks = Object.keys(cnt);
    openPop(btn, `<div class="ph">点一个标签，从选中的工程里去掉（数字 = 有几个工程带着它）</div>` +
      (ks.length ? `<div class="tags">${ks.map(t => `<button class="tag" type="button" data-brm="${esc(t)}">${esc(t)}<span class="n">${cnt[t]}</span></button>`).join('')}</div>` : '<div class="pe">选中的工程都没有标签</div>'));
  } else if (kind === 'move') {
    openPop(btn, `<div class="ph">把 ${sel.length} 个工程移到</div>
      <div class="dests">${moveTargets().map(g => `<button class="st-row cut" type="button" data-bdest="${esc(g)}"><span class="gname">${esc(g || '（根目录）')}</span></button>`).join('')}</div>
      <div class="ph">或新建分类文件夹</div>
      <div class="addrow"><input class="inp" id="bDestIn" type="text" placeholder="例如 01_商单/客户B"><button class="btn ghost cut-line" type="button" data-bdest-new style="height:40px;--k:0px">移动</button></div>`);
  } else if (kind === 'del') {
    closePop();
    const ext = sel.filter(p => p.external), own = sel.filter(p => !p.external);
    confirmBox({
      label: 'Delete', title: own.length ? `删除 ${own.length} 个工程？` : `从启动器移除 ${ext.length} 个工程？`, yes: own.length ? '移到回收站' : '移除',
      html: (own.length ? `这些工程的整个文件夹会移到系统回收站，删错了可以从回收站还原。${listHTML(own)}` : '') +
        (ext.length ? `${own.length ? '<br>' : ''}导入的工程只是从启动器移除，文件夹和文件都不动。${listHTML(ext)}` : ''),
      onYes: async () => {
        await unimport(ext);
        report(await runBatch(own, p => invoke('delete_project', { root, path: p.path })), n => (own.length ? `已把 ${n} 个工程移到回收站` : '') + (ext.length ? `${own.length ? '，' : ''}已移除 ${ext.length} 个导入的工程（文件没有动）` : ''));
      }
    });
  } else if (kind === 'all') selectAll();
  else if (kind === 'none') clearSel();
}

const listHTML = l => `<span class="clist">${l.slice(0, 8).map(p => '「' + esc(p.name) + '」').join('')}${l.length > 8 ? ` 等 ${l.length} 个` : ''}</span>`;
async function runBatch(list, fn) {
  const ok = [], bad = [];
  for (const p of list) { try { await fn(p); ok.push(p); } catch (e) { bad.push([p, String(e)]); } }
  return { ok, bad };
}
function report({ ok, bad }, okText) {
  clearSel(); scan({ quiet: true });
  toast(okText(ok.length) + (bad.length ? `；${bad.length} 个没成功：${bad.map(([p, e]) => `「${p.name}」${e}`).join('；')}` : ''));
}

function batchStatus(st) {
  const sel = selected();
  sel.forEach(p => { p.status = st; saveMeta(p); });
  closePop(); render(); toast(`已把 ${sel.length} 个工程改为「${st}」`);
}
function batchTagAdd(v) {
  v = (v || '').trim(); if (!v) { const i = $('#bTagIn'); if (i) i.focus(); return; }
  let n = 0;
  selected().forEach(p => { if (!p.tags.includes(v)) { p.tags.push(v); saveMeta(p); n++; } });
  closePop(); render(); toast(n ? `已给 ${n} 个工程加上「${v}」` : `选中的工程都已经有「${v}」了`);
}
function batchTagRm(v) {
  let n = 0;
  selected().forEach(p => { if (p.tags.includes(v)) { p.tags = p.tags.filter(x => x !== v); saveMeta(p); n++; } });
  if (ui.tag === v && !projects.some(p => p.tags.includes(v))) ui.tag = '';
  closePop(); render(); toast(`已从 ${n} 个工程里去掉「${v}」`);
}
function askBatchMove(dest) {
  const sel = selected(); closePop();
  confirmBox({
    label: 'Move', title: `移动 ${sel.length} 个工程？`, yes: '移动',
    html: `移到 <b>${esc(dest || '根目录')}</b>，这个分类文件夹不存在会自动新建。<br>移动的是真实文件夹；目标里有同名文件夹、或工程正在 TD 里打开的会跳过。${sel.some(p => p.external) ? '<br>其中导入的工程会从原来的位置搬进工程根目录，和根目录不在同一块硬盘的会跳过。' : ''}${listHTML(sel)}`,
    onYes: async () => report(await runBatch(sel, async p => { await invoke('move_project', { root, path: p.path, dest, external: p.external }); if (p.external) dropImport(p.path); }), n => `已移动 ${n} 个工程`)
  });
}

/* ================= 老工程补标注（企划 §6.2） ================= */
const groupTag = p => { const g = topGroup(p); return g ? (g.replace(/^\d+[\s_.\-]*/, '') || g) : ''; };
let annStatus = '已完成';
function openAnn() {
  const list = projects.filter(p => !p.annotated);
  if (!list.length) { toast('所有工程都已经有标注了'); return; }
  closeAll(); clearSel();
  const row = p => `<div class="ann-row" data-aid="${esc(p.id)}">
      <input type="checkbox" checked aria-label="写入 ${esc(p.name)}">
      <input class="inp a-name" type="text" value="${esc(p.name)}" aria-label="工程名">
      <input class="inp a-date mono" type="text" value="${esc(p.created)}" aria-label="创建日期">
      <div class="sub">${esc((p.group ? p.group + '/' : '') + p.folder)}${p.toe ? ' · ' + esc(p.toe.split(/[\\/]/).pop()) : ''}<span class="gt"></span></div></div>`;
  const sec = (title, sub, arr) => arr.length ? `<div class="ann-sec"><h3><span>${title}</span><span class="label">${arr.length}</span></h3><p class="ann-sub">${sub}</p>${arr.map(row).join('')}</div>` : '';
  $('#aList').innerHTML =
    sec('很可能是工程', '有 TD 自带的工程结构（Backup、Audio、Geo…）或本模板的结构', list.filter(p => p.confidence === 'likely')) +
    sec('疑似', '只有零散的 .toe，没有工程结构，确认一下是不是工程', list.filter(p => p.confidence !== 'likely'));
  $('#aList').style.display = 'flex'; $('#aList').style.flexDirection = 'column'; $('#aList').style.gap = '18px';
  renderAnnOpts(); updateAnn();
  $('#scrim').classList.add('on'); $('#ann').classList.add('on');
}
function renderAnnOpts() {
  $('#aStatus').innerHTML = STATUSES.map(s => `<button type="button" data-ast="${s}" aria-pressed="${annStatus === s}">${s}</button>`).join('');
}
function updateAnn() {
  const rows = [...document.querySelectorAll('#aList .ann-row')];
  let on = 0;
  rows.forEach(r => {
    const c = r.querySelector('input[type=checkbox]').checked;
    r.classList.toggle('off', !c); if (c) on++;
    const p = projects.find(x => x.id === r.dataset.aid);
    const t = $('#aGroupTag').checked && p ? groupTag(p) : '';
    r.querySelector('.gt').textContent = t ? ' · 标签：' + t : '';
  });
  const off = rows.length - on;
  $('#aGo').textContent = off ? `写入 ${on} 个 · 忽略 ${off} 个` : `写入 ${on} 个`;
}
$('#aList').addEventListener('change', updateAnn);
$('#aGroupTag').addEventListener('change', updateAnn);
$('#aGo').addEventListener('click', async () => {
  const rows = [...document.querySelectorAll('#aList .ann-row')];
  let n = 0, ign = 0; const bad = [];
  $('#aGo').disabled = true;
  for (const r of rows) {
    const p = projects.find(x => x.id === r.dataset.aid); if (!p) continue;
    if (!r.querySelector('input[type=checkbox]').checked) { ignored.add(p.path); ign++; continue; }
    const name = r.querySelector('.a-name').value.trim() || p.name;
    const created = r.querySelector('.a-date').value.trim();
    const tags = [...p.tags], gt = $('#aGroupTag').checked ? groupTag(p) : '';
    if (gt && !tags.includes(gt)) tags.push(gt);
    try { await invoke('save_meta', { path: p.path, meta: { name, tags, status: annStatus, client: '', note: '', created } }); n++; }
    catch (e) { bad.push(`「${p.name}」${e}`); }
  }
  saveIgnored(); $('#aGo').disabled = false; closeAll();
  await scan({ quiet: true });
  toast(`已给 ${n} 个工程写入标注${ign ? `，忽略 ${ign} 个（侧栏可恢复）` : ''}${bad.length ? `；${bad.length} 个失败：${bad.join('；')}` : ''}`);
});

/* ================= 导入工程（企划 §3） ================= */
const baseName = p => String(p).replace(/[\\/]+$/, '').split(/[\\/]/).pop();
let impList = [];
async function pickImport() {
  if (!root) { toast('先选择工程根目录'); pickRoot(); return; }
  const dirs = await dialog.open({ directory: true, multiple: true, title: '选择要导入的 TD 工程文件夹（选上级文件夹可以批量导入）' });
  if (!dirs) return;
  importPaths(Array.isArray(dirs) ? dirs : [dirs]);
}
// 选的文件夹本身是工程就直接导入；是上级文件夹就列出里面的工程让你勾选
async function importPaths(paths) {
  if (!root) { toast('先选择工程根目录，再导入'); return; }
  const found = [], notes = [];
  for (const path of paths) {
    try {
      const r = await invoke('inspect_import', { root, path });
      if (r.insideRoot) notes.push(`「${baseName(path)}」已经在工程根目录里，不用导入`);
      else if (!r.projects.length) notes.push(`「${baseName(path)}」里没有找到 TD 工程`);
      else found.push({ direct: r.projects.length === 1 && r.projects[0].path === path, list: r.projects });
    } catch (e) { notes.push(String(e)); }
  }
  const seen = new Set(projects.map(p => p.path)), fresh = [];
  let dup = 0;
  for (const f of found) for (const raw of f.list) {
    if (seen.has(raw.path)) { dup++; continue; }
    seen.add(raw.path); fresh.push(Object.assign(normalize(raw), { direct: f.direct }));
  }
  if (dup) notes.push(`${dup} 个已经在列表里`);
  if (!fresh.length) { toast(notes.join('；') || '没有可以导入的工程'); return; }
  if (fresh.every(p => p.direct)) { doImport(fresh.map(p => ({ p, name: p.name, created: p.created })), notes); return; }
  openImp(fresh, notes);
}
function openImp(list, notes) {
  closeAll(); clearSel(); impList = list;
  $('#iNote').hidden = !notes.length; $('#iNote').textContent = notes.join('；');
  $('#iList').innerHTML = `<div class="ann-sec">${list.map(p => `<div class="ann-row" data-iid="${esc(p.id)}">
      <input type="checkbox" checked aria-label="导入 ${esc(p.name)}">
      <input class="inp a-name" type="text" value="${esc(p.name)}" aria-label="工程名"${p.hasMeta ? ' disabled title="已经有 project.json，沿用里面的名字"' : ''}>
      <input class="inp a-date mono" type="text" value="${esc(p.created)}" aria-label="创建日期"${p.hasMeta ? ' disabled' : ''}>
      <div class="sub">${esc(shortPath(p.path))}${p.toe ? ' · ' + esc(baseName(p.toe)) : ' · 没有 .toe'}${p.hasMeta ? ' · 已有标注' : ''}</div></div>`).join('')}</div>`;
  updateImp();
  $('#scrim').classList.add('on'); $('#imp').classList.add('on');
}
function updateImp() {
  let on = 0;
  document.querySelectorAll('#iList .ann-row').forEach(r => { const c = r.querySelector('input[type=checkbox]').checked; r.classList.toggle('off', !c); if (c) on++; });
  $('#iGo').textContent = `导入 ${on} 个`; $('#iGo').disabled = !on;
}
$('#iList').addEventListener('change', updateImp);
$('#iGo').addEventListener('click', () => {
  const picked = [];
  document.querySelectorAll('#iList .ann-row').forEach(r => {
    const p = impList.find(x => x.id === r.dataset.iid);
    if (p && r.querySelector('input[type=checkbox]').checked) picked.push({ p, name: r.querySelector('.a-name').value.trim() || p.name, created: r.querySelector('.a-date').value.trim() });
  });
  closeAll(); doImport(picked, []);
});
async function doImport(items, notes) {
  let n = 0; const bad = [];
  for (const { p, name, created } of items) {
    try {
      // 还没有标注的：新增一个 project.json；已有的原样不动
      if (!p.hasMeta) await invoke('save_meta', { path: p.path, meta: { name, tags: [], status: '', client: '', note: '', created } });
      if (!imports.some(x => x.path === p.path)) imports.push({ path: p.path, group: p.group || parentName(p.path) });
      if (ignored.delete(p.path)) saveIgnored();
      n++;
    } catch (e) { bad.push(`「${p.name}」${e}`); }
  }
  saveImports();
  await scan({ quiet: true });
  toast(`已导入 ${n} 个工程，文件留在原处` + (bad.length ? `；${bad.length} 个没成功：${bad.join('；')}` : '') + (notes.length ? '；' + notes.join('；') : ''));
}
// 把文件夹（或 .toe）拖进窗口也能导入
(() => {
  const wv = window.__TAURI__.webview && window.__TAURI__.webview.getCurrentWebview && window.__TAURI__.webview.getCurrentWebview();
  if (!wv || !wv.onDragDropEvent) return;
  wv.onDragDropEvent(ev => {
    const t = ev.payload && ev.payload.type;
    if (t === 'enter' || t === 'over') document.body.classList.add('dropping');
    else document.body.classList.remove('dropping');
    if (t === 'drop' && ev.payload.paths && ev.payload.paths.length) importPaths(ev.payload.paths);
  }).catch(() => {});
})();

/* ================= 自定义标志 ================= */
// 文件夹图标的画法（256 的画布上）：深色的背板 + 浅色的前板 + 居中的标志。和自带的 folder.ico 是同一个样子
const DEFAULT_MARK = (() => { const p = document.querySelector('header svg.logo path'); return p ? p.getAttribute('d') : ''; })();
function folderCanvas(size, logo, dark) {
  const c = document.createElement('canvas'); c.width = c.height = size;
  const g = c.getContext('2d'); g.scale(size / 256, size / 256);
  g.fillStyle = '#45453F';
  g.fill(new Path2D('M20 60a10 10 0 0 1 10-10h62a10 10 0 0 1 8 4l14 18h112a10 10 0 0 1 10 10v126a10 10 0 0 1-10 10h-196a10 10 0 0 1-10-10z'));
  g.beginPath(); g.roundRect(12, 92, 232, 134, 11);
  g.fillStyle = dark ? '#1C1C1C' : '#F4F4F1'; g.fill();
  g.lineWidth = 4; g.strokeStyle = '#45453F'; g.stroke();
  if (logo) {
    // 标志放进前板中间 168×100 的范围里，保持比例
    const k = Math.min(168 / logo.width, 100 / logo.height), w = logo.width * k, h = logo.height * k;
    g.imageSmoothingQuality = 'high';
    g.drawImage(logo, 128 - w / 2, 159 - h / 2, w, h);
  } else if (DEFAULT_MARK) {
    g.translate(128, 159); g.scale(0.142, 0.142); g.translate(-640 - 57, -640);
    g.fillStyle = '#111111'; g.fill(new Path2D(DEFAULT_MARK), 'evenodd');
  }
  return c;
}
const canvasPng = c => new Promise((ok, no) => c.toBlob(b => b ? b.arrayBuffer().then(a => ok(new Uint8Array(a))) : no('图片导出失败'), 'image/png'));
// 把几张 PNG 打包成一个 .ico（文件头 6 字节 + 每张 16 字节的目录 + PNG 内容）
function buildIco(items) {
  const head = 6 + 16 * items.length, out = new Uint8Array(head + items.reduce((n, i) => n + i.png.length, 0)), v = new DataView(out.buffer);
  v.setUint16(2, 1, true); v.setUint16(4, items.length, true);
  let off = head;
  items.forEach((it, i) => {
    const e = 6 + 16 * i;
    out[e] = it.size >= 256 ? 0 : it.size; out[e + 1] = it.size >= 256 ? 0 : it.size;
    v.setUint16(e + 4, 1, true); v.setUint16(e + 6, 32, true);
    v.setUint32(e + 8, it.png.length, true); v.setUint32(e + 12, off, true);
    out.set(it.png, off); off += it.png.length;
  });
  return out;
}
// 把选的图片裁掉四周的透明边、缩进 512 以内，并看它是深色还是浅色（浅色标志配深色前板）
function prepLogo(img, vector) {
  // 矢量图（SVG）没有固定像素，按 512 画；位图只缩小不放大
  const W = img.naturalWidth || img.width || 512, H = img.naturalHeight || img.height || 512, k = vector ? 512 / Math.max(W, H) : Math.min(1, 512 / Math.max(W, H));
  const c = document.createElement('canvas'); c.width = Math.max(1, Math.round(W * k)); c.height = Math.max(1, Math.round(H * k));
  const g = c.getContext('2d'); g.drawImage(img, 0, 0, c.width, c.height);
  const d = g.getImageData(0, 0, c.width, c.height).data;
  let x0 = c.width, y0 = c.height, x1 = -1, y1 = -1, lum = 0, n = 0;
  for (let y = 0; y < c.height; y++) for (let x = 0; x < c.width; x++) {
    const i = (y * c.width + x) * 4, a = d[i + 3];
    if (a < 16) continue;
    if (x < x0) x0 = x; if (x > x1) x1 = x; if (y < y0) y0 = y; if (y > y1) y1 = y;
    lum += (0.2126 * d[i] + 0.7152 * d[i + 1] + 0.0722 * d[i + 2]) * a; n += a;
  }
  if (x1 < 0) throw '这张图片是全透明的，换一张';
  const t = document.createElement('canvas'); t.width = x1 - x0 + 1; t.height = y1 - y0 + 1;
  t.getContext('2d').drawImage(c, x0, y0, t.width, t.height, 0, 0, t.width, t.height);
  return { logo: t, dark: lum / n > 170 };
}
const ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256];
let customLogo = null; // { path, src }：正在用的自定义标志
function showLogo(info) {
  customLogo = info;
  const svg = document.querySelector('header svg.logo'); let img = document.querySelector('header img.logo');
  // 优先级：自己选的标志 > 主题自带的标志 > 默认标志
  const src = info ? info.src : themeLogo(curTheme());
  if (src) {
    if (!img) { img = document.createElement('img'); img.className = 'logo'; img.alt = ''; svg.after(img); }
    img.src = src; svg.style.display = 'none';
  } else { if (img) img.remove(); svg.style.display = ''; }
}
async function loadLogo() {
  try { const r = await invoke('custom_logo'); showLogo(r ? { path: r[0], src: convertFileSrc(r[0]) + '?v=' + r[1] } : null); } catch (e) { showLogo(null); }
}
function drawLogoPrev() {
  const c = $('#sLogoPrev'), g = c.getContext('2d'); g.clearRect(0, 0, c.width, c.height);
  $('#sLogoState').textContent = customLogo ? '正在使用自定义标志' : '正在使用默认标志';
  $('#sLogoReset').hidden = !customLogo;
  if (!customLogo) { g.drawImage(folderCanvas(c.width, null, false), 0, 0); return; }
  // 预览要读像素，所以从后端拿图片内容再画（直接用文件地址画，画布会被浏览器锁住）
  invoke('read_image', { path: customLogo.path }).then(b => {
    const url = URL.createObjectURL(new Blob([new Uint8Array(b)], { type: 'image/png' })), img = new Image();
    img.onload = () => { try { const p = prepLogo(img); g.clearRect(0, 0, c.width, c.height); g.drawImage(folderCanvas(c.width, p.logo, p.dark), 0, 0); } catch (e) {} URL.revokeObjectURL(url); };
    img.src = url;
  }).catch(() => {});
}
async function pickLogo() {
  const f = await dialog.open({ multiple: false, title: '选择标志图片', filters: [{ name: '图片', extensions: ['png', 'svg', 'jpg', 'jpeg', 'webp'] }] });
  if (!f) return;
  const path = Array.isArray(f) ? f[0] : f, btn = $('#sLogoPick'); btn.disabled = true;
  try {
    const bytes = new Uint8Array(await invoke('read_image', { path }));
    const type = /\.svg$/i.test(path) ? 'image/svg+xml' : /\.jpe?g$/i.test(path) ? 'image/jpeg' : /\.webp$/i.test(path) ? 'image/webp' : 'image/png';
    const url = URL.createObjectURL(new Blob([bytes], { type }));
    const img = await new Promise((ok, no) => { const i = new Image(); i.onload = () => ok(i); i.onerror = () => no('这张图片打不开，换一张试试'); i.src = url; });
    const { logo, dark } = prepLogo(img, type === 'image/svg+xml'); URL.revokeObjectURL(url);
    const items = [];
    for (const size of ICO_SIZES) items.push({ size, png: await canvasPng(folderCanvas(size, logo, dark)) });
    // 左上角用的标志：放进 256 高以内的透明画布
    const k = Math.min(1, 256 / logo.height), lc = document.createElement('canvas');
    lc.width = Math.max(1, Math.round(logo.width * k)); lc.height = Math.max(1, Math.round(logo.height * k));
    lc.getContext('2d').drawImage(logo, 0, 0, lc.width, lc.height);
    await invoke('save_custom_logo', { logo: Array.from(await canvasPng(lc)), ico: Array.from(buildIco(items)) });
    await loadLogo(); drawLogoPrev();
    const redo = cfg.folderIcon && projects.some(p => p.folderIcon);
    toast('标志已更换' + (redo ? '，正在更新工程文件夹的图标' : ''));
    if (cfg.folderIcon) { iconFailed.clear(); syncIcons(true, false, true); }
  } catch (e) { toast('更换失败：' + e); }
  finally { btn.disabled = false; }
}
async function resetLogo() {
  try {
    await invoke('clear_custom_logo'); await loadLogo(); drawLogoPrev();
    toast('已恢复默认标志');
    if (cfg.folderIcon) { iconFailed.clear(); syncIcons(true, false, true); }
  } catch (e) { toast('恢复失败：' + e); }
}

/* ================= 设置 ================= */
function renderSetInfo() {
  const n = projects.filter(p => p.external).length;
  $('#sImports').innerHTML = imports.length
    ? `登记了 ${imports.length} 个根目录以外的工程${ui.missingN ? `，其中 ${ui.missingN} 个现在读不到 · <button type="button" data-unmissing>不再登记读不到的</button>` : ''}。它们的文件留在原处，卡片上标着"外部"，按原来所在的文件夹分组（现在读得到 ${n} 个）。`
    : '还没有导入过根目录以外的工程。点右上角"导入"，或者把工程文件夹拖进窗口。';
}
const MODES = [['light', '浅色'], ['dark', '深色'], ['auto', '跟随系统']];
function renderThemes() {
  const cur = curTheme().id;
  $('#sTheme').innerHTML = themes.map(t => `<button type="button" data-theme-id="${esc(t.id)}" aria-pressed="${t.id === cur}"><b>${esc(t.name)}</b>${t.description ? `<small>${esc(t.description)}</small>` : ''}${t.builtin ? '' : '<small class="mine">自制</small>'}</button>`).join('');
  $('#sMode').innerHTML = MODES.map(([id, name]) => `<button type="button" data-theme-mode="${id}" aria-pressed="${id === themeMode}">${name}</button>`).join('');
  $('#sThemeErr').hidden = !themeErrors.length;
  $('#sThemeErr').innerHTML = themeErrors.length ? '这些主题文件没读成功：<br>' + themeErrors.map(esc).join('<br>') : '';
}
function setTheme(id) {
  theme = id; try { localStorage.setItem('theme', id); } catch (e) {}
  applyTheme(); showLogo(customLogo); renderThemes();
}
function setThemeMode(m) {
  themeMode = m; try { localStorage.setItem('themeMode', m); } catch (e) {}
  applyTheme(); showLogo(customLogo); renderThemes();
}
async function openThemesDir() {
  try { await invoke('open_themes_dir'); toast('主题文件夹已打开：里面有《主题制作规范》和一个模板，做好后放进去，再点"重新读取"'); }
  catch (e) { toast('打不开主题文件夹：' + e); }
}
async function reloadThemes() { await loadThemes(); renderThemes(); toast(`读到 ${themes.length} 个主题` + (themeErrors.length ? `，${themeErrors.length} 个文件没读成功` : '')); }
function openSet() {
  closeAll(); syncRoot(); renderThemes();
  $('#sIcon').checked = cfg.folderIcon && IS_WIN; $('#sIcon').disabled = !IS_WIN;
  $('#sIconRow').classList.toggle('off', !IS_WIN);
  if (!IS_WIN) $('#sIconRow').title = '目前只支持 Windows';
  $('#sDate').checked = cfg.datePrefix; $('#sOpen').checked = cfg.autoOpen; $('#sRescan').checked = cfg.autoRescan; $('#sWm').checked = cfg.autoWm; loadComps().then(renderSetComps); $('#sGroup').checked = cfg.groupByFolder;
  renderSetInfo(); drawLogoPrev();
  $('#scrim').classList.add('on'); $('#set').classList.add('on');
}
$('#sIcon').addEventListener('change', e => { setCfg('folderIcon', e.target.checked); if (e.target.checked) iconFailed.clear(); syncIcons(e.target.checked, false); });
$('#sDate').addEventListener('change', e => setCfg('datePrefix', e.target.checked));
$('#sOpen').addEventListener('change', e => setCfg('autoOpen', e.target.checked));
$('#sRescan').addEventListener('change', e => setCfg('autoRescan', e.target.checked));
$('#sWm').addEventListener('change', e => { setCfg('autoWm', e.target.checked); if (e.target.checked) wmAuto(); });
$('#sGroup').addEventListener('change', e => { setCfg('groupByFolder', e.target.checked); render(); });

/* ================= 通用 ================= */
/* ================= 刷新 ================= */
$('#doRefresh').addEventListener('click', async () => { await scan(); if (!ui.error) toast(`已刷新 · 共 ${projects.length} 个工程`); });

/* ================= 组件库：新建工程时可以勾选带上的 tox ================= */
// 每个组件：src 是一个 .tox 文件或一个文件夹，新建时整个复制进工程的 tox 文件夹；
// load 是文件夹里要自动载入的 .tox。清单会记进 project.json，由工程里的载入器在第一次打开时载入。
let comps = null, compSel = new Set();
function saveComps() { try { localStorage.setItem('components', JSON.stringify(comps.map(({ missing, ...c }) => c))); } catch (e) {} }
async function loadComps() {
  if (!comps) {
    try { const v = JSON.parse(localStorage.getItem('components') || 'null'); if (Array.isArray(v)) comps = v; } catch (e) {}
    if (!comps) {
      if (!root) return;
      comps = [{ id: 'thumbnail', name: '缩略图截取', src: 'builtin:thumbnail', load: [], on: true }];
      // 第一次：按库里现成的位置找一遍，找到的才加进来
      const guess = [['TDMCP', '00_Library/TDMCP/TDMCP.tox', [], true], ['Mediapipe', '00_Library/Mediapipe_release_0.6.0/toxes', ['MediaPipe.tox', 'hand_tracking.tox'], false], ['BTDR', '03_BTDR/release_BetterTDRender/BTDR.tox', [], false]];
      for (const [name, rel, load, on] of guess) {
        const src = root.replace(/[\\/]+$/, '') + '/' + rel;
        if (await invoke('path_kind', { path: src }).catch(() => '')) comps.push({ id: name, name, src, load, on });
      }
      saveComps();
    }
  }
  for (const c of comps) c.missing = !c.src.startsWith('builtin:') && !(await invoke('path_kind', { path: c.src }).catch(() => ''));
}
const compNote = c => c.src.startsWith('builtin:') ? '软件自带 · 保存工程时自动截一张预览图' : c.missing ? '找不到文件：' + c.src : shortPath(c.src) + (c.load && c.load.length ? ' · 自动载入 ' + c.load.join('、') : '');
function renderNpComps() {
  $('#npComps').innerHTML = (comps || []).map(c => `<label class="comp-row"><input type="checkbox" data-comp="${esc(c.id)}"${compSel.has(c.id) ? ' checked' : ''}${c.missing ? ' disabled' : ''}><span>${esc(c.name)}</span><span></span><span class="sub${c.missing ? ' bad' : ''}">${esc(compNote(c))}</span></label>`).join('') || '<div class="set-info">组件库是空的，可以在设置里添加</div>';
}
$('#npComps').addEventListener('change', e => { const id = e.target.dataset.comp; if (!id) return; e.target.checked ? compSel.add(id) : compSel.delete(id); });
function renderSetComps() {
  $('#sComps').innerHTML = (comps || []).map(c => `<div class="comp-row"><input type="checkbox" data-comp-on="${esc(c.id)}"${c.on ? ' checked' : ''} title="新建工程时默认勾选" aria-label="${esc(c.name)} 默认勾选"><span>${esc(c.name)}</span>${c.src.startsWith('builtin:') ? '<span></span>' : `<button type="button" data-comp-rm="${esc(c.id)}">移除</button>`}<span class="sub${c.missing ? ' bad' : ''}">${esc(compNote(c))}</span></div>`).join('') || '<div class="set-info">还没有组件</div>';
}
$('#sComps').addEventListener('change', e => { const c = comps.find(x => x.id === e.target.dataset.compOn); if (c) { c.on = e.target.checked; saveComps(); } });
$('#sComps').addEventListener('click', e => { const id = e.target.dataset.compRm; if (!id) return; comps = comps.filter(c => c.id !== id); saveComps(); renderSetComps(); });
async function addComp(dir) {
  const f = await dialog.open(dir ? { directory: true, multiple: false, title: '选择组件文件夹（新建工程时整个复制进 tox 文件夹）' } : { multiple: false, title: '选择 .tox 文件', filters: [{ name: 'TouchDesigner 组件', extensions: ['tox'] }] });
  if (!f) return;
  await loadComps(); if (!comps) comps = [];
  const name = baseName(f).replace(/\.tox$/i, '');
  if (comps.some(c => normPath(c.src) === normPath(f))) { toast('这个组件已经在库里了'); return; }
  comps.push({ id: name + '-' + Date.now().toString(36), name, src: f, load: [], on: false });
  saveComps(); renderSetComps(); toast(`已加入组件库：${name}`);
}
$('#sCompFile').addEventListener('click', () => addComp(false));
$('#sCompDir').addEventListener('click', () => addComp(true));

/* 新建时选分类：根目录直属、里面已经有工程的文件夹 */
function renderCats() {
  const cats = [...new Set(projects.filter(p => !p.external).map(topGroup).filter(Boolean))].sort((a, b) => a.localeCompare(b, 'zh'));
  let last = ''; try { last = localStorage.getItem('newCat') || ''; } catch (e) {}
  if (ui.group && cats.includes(ui.group)) last = ui.group;
  $('#npCat').innerHTML = `<option value="">（根目录）</option>` + cats.map(c => `<option value="${esc(c)}"${c === last ? ' selected' : ''}>${esc(c)}</option>`).join('');
  $('#npCase').checked = /案例/.test($('#npCat').value);
}
$('#npCat').addEventListener('change', e => { try { localStorage.setItem('newCat', e.target.value); } catch (er) {} $('#npCase').checked = /案例/.test(e.target.value); updateTree(); });
$('#npCase').addEventListener('change', updateTree);

/* ================= 案例工程规范检查（详情里，只对「案例」分类下的工程显示） ================= */
// 官网用的分类：取值固定，和官网报价的档位一致（见《案例工程规范》8.3）
const CASE_OPTS = {
  interact: [['none', '无交互'], ['simple', '声音 / 麦克风 / 鼠标键盘 / MIDI'], ['camera', '摄像头（动作、轮廓、颜色）'], ['tracking', '人体 / 手势识别'], ['sensor', '硬件传感器（Arduino 等）'], ['depth', '深度相机（Kinect、RealSense）'], ['external', '外部联动（手机、网站、别的软件）']],
  visual: [['basic', '2D 图形、滤镜、文字'], ['particle', '粒子、噪声、音频可视化'], ['advanced', '流体、复杂粒子、多层叠加'], ['scene3d', '三维模型、灯光、镜头'], ['shader', '自写 GLSL'], ['photoreal', '写实']],
  output: [['screen', '一块屏幕'], ['projector', '一台投影'], ['led', 'LED 大屏'], ['multi', '多屏 / 多投影拼接'], ['mapping', '投影映射']],
  platform: [['td', 'TouchDesigner'], ['unity', 'Unity'], ['ue', 'Unreal'], ['xr', 'XR']],
};
const CASE_BAN = ['毕设', '课设', '代做', '代写'];
const listOf = v => String(v || '').split(/[,，、\s]+/).map(x => x.trim()).filter(Boolean);
const isCase = p => /案例/.test(topGroup(p)) || /^CASE\d/i.test(p.folder || '');
// 工程类型（规范 v2.0）：case 案例工程 / order 定制工程。没写的按所在分类猜，个人作品之类不算这两种
const projType = p => (p.meta && (p.meta.type === 'case' || p.meta.type === 'order') && p.meta.type) || (isCase(p) ? 'case' : /商单|交付/.test(topGroup(p)) ? 'order' : '');
// 已经标注过、但还没写类型的工程，补上（只补一次，写不进去就算了）
const typeTried = new Set();
function fillTypes() {
  for (const p of projects) {
    const t = projType(p);
    if (!t || !p.hasMeta || (p.meta && p.meta.type) || typeTried.has(p.path)) continue;
    typeTried.add(p.path); p.meta = { ...(p.meta || {}), type: t };
    invoke('save_meta', { path: p.path, meta: { type: t } }).catch(() => {});
  }
}
// 下一个没用过的案例编号：现有工程用过的、以前发过的（哪怕工程已删、已下架）都算用过
function nextCaseNo() {
  let hi = 0; try { hi = Number(localStorage.getItem('caseHigh')) || 0; } catch (e) {}
  for (const p of projects) {
    const a = ((p.meta && p.meta.caseId || '').match(/^case(\d+)$/) || [])[1], b = ((p.folder || '').match(/^CASE[\s_-]*(\d+)/i) || [])[1];
    hi = Math.max(hi, Number(a) || 0, Number(b) || 0);
  }
  return hi + 1;
}
const VER_RE = /^\d+\.\d+$/;
// 官网后台会拒收的问题，先在这里查一遍（规则同官网对接文档 2.4）
function caseWebIssues(p) {
  const m = p.meta || {}, out = [], len = v => [...String(v || '')].length;
  if (!/^case\d{2,}$/.test(m.caseId || '')) out.push('编号要写成 case06 这样');
  if (!/^[a-z0-9]+(-[a-z0-9]+){0,3}$/.test(m.slug || '')) out.push('英文短名只用小写英文和数字，词之间用连字符，最多 4 个词');
  if (!p.name || len(p.name) > 12) out.push('标题要在 12 个字以内');
  if (!m.subtitle || len(m.subtitle) > 40) out.push('副标题必填，40 个字以内');
  if (!p.note || len(p.note) > 60) out.push('备注（简介）要填，60 个字以内');
  if (!p.tags.length) out.push('至少要有 1 个技术标签');
  if (!m.tdVersion) out.push('还没填 TD 版本号');
  if (!VER_RE.test(m.version || '')) out.push('工程版本要写成 1.0 这样');
  for (const k of ['visual', 'output', 'platform']) if (m[k] && !CASE_OPTS[k].some(o => o[0] === m[k])) out.push(`${k} 的值官网不认识：${m[k]}`);
  if ((m.interact || []).some(v => !CASE_OPTS.interact.some(o => o[0] === v))) out.push('interact 里有官网不认识的值');
  const text = [p.name, m.subtitle, p.note, ...(m.keywords || [])].join(' '), hit = CASE_BAN.filter(w => text.includes(w));
  if (hit.length) out.push(`文案里有不能出现的词：${hit.join('、')}`);
  if (m.fromOrder && !m.clientConsent) out.push('来自定制单，还没确认客户同意，官网不会公开');
  return out;
}
// 不拦着上传、但会影响效果的
function caseWebHints(p) {
  const m = p.meta || {};
  return !(Array.isArray(m.interact) && m.interact.length) || !m.visual ? ['交互方式和视觉类型没选：能上传，但不会出现在官网对话的「类似这种」里'] : [];
}
const fmtMB = b => (b / 1048576).toFixed(b < 10485760 ? 1 : 0) + ' MB';
async function renderCase(p) {
  const box = $('#dCase'), type = projType(p);
  if (!type) { box.hidden = true; return; }
  const m = p.meta || {};
  const txt = (key, label, ph, val, lock) => `<div class="field"><label>${label}</label><input class="inp${/Id|slug|Version|version/.test(key) ? ' mono' : ''}" type="text" data-case="${key}" placeholder="${esc(ph)}" value="${esc(val == null ? '' : val)}"${lock ? ' disabled' : ''}></div>`;
  if (type === 'order') {
    let olist = [];
    try { olist = await invoke('check_case', { path: p.path, kind: 'order' }); } catch (e) {}
    if (ui.openId !== p.id) return;
    const on = olist.filter(x => x.ok).length;
    box.hidden = false;
    box.innerHTML = `<div class="fl"><span>交付检查</span><span class="label">${on} / ${olist.length} · 定制工程</span></div>
      <ul class="case-list">${olist.map(x => `<li class="${x.ok ? 'yes' : 'no'}"><span>${x.ok ? '✓' : '○'}</span><span>${esc(x.label)}</span>${x.ok ? '' : `<small>${esc(x.hint)}</small>`}</li>`).join('')}</ul>
      <div class="case-form" style="margin-top:12px">${txt('slug', '英文项目名', 'four-relics', m.slug)}${txt('version', '工程版本', '1.0', m.version)}${txt('tdVersion', 'TD 版本', '2025.33230', m.tdVersion)}${txt('orderId', '官网订单号', 'TDX-1126-9404，有就填', m.orderId)}</div>
      <div class="theme-acts" style="margin-top:8px"><button class="btn text" type="button" data-case-act="pack">打包交付</button><small style="font-size:12px;color:var(--mute)">存到工程的 deliver 文件夹，不带 project.json、docs、Backup</small></div>`;
    return;
  }
  let list = [];
  try { list = await invoke('check_case', { path: p.path }); } catch (e) { box.hidden = true; return; }
  if (ui.openId !== p.id) return;
  const n = list.filter(x => x.ok).length;
  const guess = ((p.folder || '').match(/^CASE[\s_-]*(\d+)/i) || [])[1];
  const sel = (key, cur) => `<select class="inp" data-case="${key}"><option value="">（没选）</option>${CASE_OPTS[key].map(([v, l]) => `<option value="${v}"${cur === v ? ' selected' : ''}>${esc(l)}</option>`).join('')}</select>`;
  const issues = caseWebIssues(p), hints = caseWebHints(p), lock = !!m.webPackedAt;
  box.hidden = false;
  box.innerHTML = `<div class="fl"><span>案例工程规范</span><span class="label">${n} / ${list.length}${n === list.length ? ' · 可以交付' : ''}</span></div>
    <ul class="case-list">${list.map(x => `<li class="${x.ok ? 'yes' : 'no'}"><span>${x.ok ? '✓' : '○'}</span><span>${esc(x.label)}</span>${x.ok ? '' : `<small>${esc(x.hint)}</small>`}</li>`).join('')}</ul>
    <div class="fl" style="margin-top:14px"><span>官网信息</span><span class="label" id="dCaseWeb"></span></div>
    <div class="case-form">
      ${txt('caseId', '编号', guess ? 'case' + guess.padStart(2, '0') : 'case06', m.caseId, lock)}${txt('slug', '英文短名', 'pixel-gem', m.slug, lock)}
      ${lock ? `<div class="field wide"><small style="font-size:12px;color:var(--mute)">已经准备过官网上传包，编号和短名锁住了：它们是官网的网址，改了官网会当成一个新案例。<button type="button" class="linkish" data-case-act="unlock">仍要修改</button></small></div>` : ''}
      ${txt('subtitle', '副标题（40 字以内）', '一句话说清交互方式', m.subtitle)}
      ${txt('tdVersion', 'TD 版本', '2025.33230', m.tdVersion)}${txt('version', '工程版本', '1.0', m.version)}
      <div class="field wide"><label>交互方式（可多选）</label><div class="case-checks">${CASE_OPTS.interact.map(([v, l]) => `<label class="chk"><input type="checkbox" data-case-int="${v}"${(m.interact || []).includes(v) ? ' checked' : ''}><span>${esc(l)}</span></label>`).join('')}</div></div>
      <div class="field"><label>视觉类型</label>${sel('visual', m.visual)}</div><div class="field"><label>输出</label>${sel('output', m.output)}</div>
      <div class="field"><label>平台</label>${sel('platform', m.platform || 'td')}</div>${txt('minGpu', '最低显卡', 'GTX 1660 或同级', m.minGpu)}
      ${txt('keywords', '客户会说的词（最多 8 个）', '文物、水墨、手势', (m.keywords || []).join('、'))}${txt('devices', '需要的外设', '摄像头；不需要就留空', (m.devices || []).join('、'))}
      <div class="field wide"><div class="case-checks">
        <label class="chk"><input type="checkbox" data-case="nonCommercial"${m.nonCommercial ? ' checked' : ''}><span>能在 TD 免费版上运行</span></label>
        <label class="chk"><input type="checkbox" data-case="fromOrder"${m.fromOrder ? ' checked' : ''}><span>来自定制单</span></label>
        <label class="chk"${m.fromOrder ? '' : ' hidden'} id="dCaseConsent"><input type="checkbox" data-case="clientConsent"${m.clientConsent ? ' checked' : ''}><span>客户已同意，且已换掉客户素材</span></label>
      </div></div>
    </div>
    <ul class="case-list" id="dCaseIssues"></ul>
    <div class="theme-acts" style="margin-top:8px"><button class="btn text" type="button" data-case-act="web">准备官网上传包</button><small style="font-size:12px;color:var(--mute)">生成到 根目录/_官网上传，只放水印版视频</small></div>`;
  paintCaseIssues(p);
}
function paintCaseIssues(p) {
  const issues = caseWebIssues(p), hints = caseWebHints(p);
  if (!$('#dCaseWeb')) return;
  $('#dCaseWeb').textContent = issues.length ? `还差 ${issues.length} 项` : '信息齐了';
  $('#dCaseIssues').innerHTML = issues.map(x => `<li class="no"><span>○</span><span>${esc(x)}</span></li>`).join('') + hints.map(x => `<li class="no"><span>·</span><span>${esc(x)}</span></li>`).join('');
}
$('#dCase').addEventListener('change', async e => {
  const p = cur(), t = e.target; if (!p) return;
  const m = p.meta = { ...(p.meta || {}) }, patch = {};
  if (t.dataset.caseInt) { patch.interact = [...$('#dCase').querySelectorAll('[data-case-int]')].filter(x => x.checked).map(x => x.dataset.caseInt); }
  else if (t.dataset.case) {
    const k = t.dataset.case;
    patch[k] = t.type === 'checkbox' ? t.checked : k === 'keywords' ? listOf(t.value).slice(0, 8) : k === 'devices' ? listOf(t.value) : k === 'caseId' || k === 'slug' ? t.value.trim().toLowerCase() : t.value.trim();
    // 版本号只收「主.次」
    if (k === 'version' && patch[k] && !VER_RE.test(patch[k])) { toast('工程版本要写成 1.0 这样（主.次）'); t.value = m.version || ''; return; }
    if (k === 'caseId' || k === 'slug') t.value = patch[k];
    if (k === 'fromOrder') { $('#dCaseConsent').hidden = !t.checked; if (!t.checked) patch.clientConsent = false; }
  } else return;
  // 类型、平台没动过也一起记下，官网读的时候就不用猜
  const type = projType(p);
  if (!m.type && type) patch.type = type;
  if (type === 'case' && !m.platform && !patch.platform) patch.platform = 'td';
  Object.assign(m, patch);
  try { await invoke('save_meta', { path: p.path, meta: patch }); saved(); } catch (er) { toast('没保存上：' + er); }
  paintCaseIssues(p);
});
$('#dCase').addEventListener('click', async e => {
  const act = e.target.dataset && e.target.dataset.caseAct, p = cur(); if (!act || !p) return;
  if (act === 'unlock') {
    confirmBox({ label: 'Warning', title: '修改编号或短名？', html: '它们是这个案例在官网的网址。改了以后再上传，官网会把它当成一个<b>新案例</b>，原来的那个还留在官网上。', yes: '仍要修改', onYes: async () => { p.meta = { ...p.meta, webPackedAt: '' }; await invoke('save_meta', { path: p.path, meta: { webPackedAt: '' } }).catch(() => {}); renderCase(p); } });
    return;
  }
  const btn = e.target; btn.disabled = true;
  try {
    if (act === 'pack') {
      const r = await invoke('pack_order', { path: p.path, date: today() });
      toast(`已打包 ${fmtMB(r.size)}，在 deliver 文件夹里`); invoke('reveal', { path: r.path }).catch(() => {}); renderCase(p);
    } else if (act === 'web') {
      const issues = caseWebIssues(p);
      if (issues.length) { toast(`官网信息还差 ${issues.length} 项，先补齐`); return; }
      toast('正在准备官网上传包，工程大的话要等一会儿…');
      const r = await invoke('web_pack', { root, path: p.path });
      const at = new Date().toISOString();
      p.meta = { ...p.meta, webPackedAt: at }; await invoke('save_meta', { path: p.path, meta: { webPackedAt: at } }).catch(() => {});
      confirmBox({ label: 'Ready', title: '官网上传包准备好了', html: `共 ${r.files.length} 个文件：<br>${r.files.map(esc).join('<br>')}<br><br>${r.notes.map(esc).join('<br>')}<br><br>把整个文件夹拖进官网后台就行。`, yes: '打开文件夹', onYes: () => invoke('reveal', { path: r.dir + '/project.json' }).catch(() => {}) });
      renderCase(p);
    }
  } catch (er) {
    const lines = String(er).split('\n');
    if (lines.length > 1 || act === 'web') confirmBox({ label: 'Not ready', title: '还不能生成', html: lines.map(esc).join('<br>'), yes: '知道了', onYes: () => {} });
    else toast(String(er));
  } finally { btn.disabled = false; }
});

/* ================= 案例视频水印 ================= */
// 四套标准模板：画布尺寸 + 画面内框。画面等比缩放放进内框，四周是 TD学长 的深色边框和标识。
const WM_TPL = {
  '16:9': { cw: 1920, ch: 1080, ix: 82, iy: 28, iw: 1756, ih: 988 },
  '9:16': { cw: 1080, ch: 1920, ix: 40, iy: 40, iw: 1000, ih: 1736 },
  '3:4': { cw: 1080, ch: 1440, ix: 40, iy: 40, iw: 1000, ih: 1256 },
  '4:3': { cw: 1440, ch: 1080, ix: 62, iy: 28, iw: 1316, ih: 988 },
};
const WM_BRAND = { name: 'TD学长', site: 'TDXUEZHANG.COM', accent: '#C8FA1E', bg: '#0A0A0A' };
const WM_MARK = (document.querySelector('header .logo path') || { getAttribute: () => '' }).getAttribute('d');
const wmPick = (w, h) => Object.keys(WM_TPL).map(k => { const [a, b] = k.split(':').map(Number); return [Math.abs(Math.log((w / h) / (a / b))), k]; }).sort((x, y) => x[0] - y[0])[0][1];
// 文件名 → 编号、标题、副标题。两种写法都认：
//   case06_pixel-gem_16x9_v2（官网用的规范写法：标题取同编号案例工程的名字）
//   CASE01_手势寻宝_握拳收集（中文直接写在文件名里）
function wmInfo(it) {
  let stem = it.name.replace(/[_\s-]*TD学长$/i, ''), no = '';
  const m = stem.match(/^CASE[\s_-]*(\d+)[\s_-]*(.*)$/i);
  if (m) { no = m[1].padStart(2, '0'); stem = m[2]; }
  const parts = stem.split('_').map(x => x.trim()).filter(x => x && !/^(16x9|9x16|3x4|4x3|v\d+)$/i.test(x));
  const proj = no && (projects.find(p => p.meta && /^case\d+$/.test(p.meta.caseId || '') && Number(p.meta.caseId.slice(4)) === Number(no)) || projects.find(p => new RegExp('^CASE[\\s_-]*0*' + Number(no) + '(\\D|$)', 'i').test(p.folder || '')));
  // 同编号的案例工程有标签的话，拿来当右下角的技术标签
  const tags = (proj && proj.tags.length ? proj.tags.slice(0, 3) : ['TouchDesigner']).map(t => t.toUpperCase());
  const ascii = !/[^\x00-\x7f]/.test(parts.join(''));
  if (proj && ascii) return { no, title: proj.name || parts[0] || it.name, sub: (proj.meta && proj.meta.subtitle) || '', tags };
  return { no, title: (parts[0] || it.name).replace(/-/g, ascii ? ' ' : '-'), sub: parts.slice(1).join(' '), tags };
}
function wmDraw(t, info) {
  const c = document.createElement('canvas'); c.width = t.cw; c.height = t.ch;
  const g = c.getContext('2d'), B = WM_BRAND, mono = '"Cascadia Mono","Consolas","SF Mono","Menlo",monospace', cjk = '"Microsoft YaHei UI","Microsoft YaHei","PingFang SC","Noto Sans SC",sans-serif';
  const { ix, iy, iw, ih, cw, ch } = t, ir = ix + iw, ib = iy + ih, tall = ch - ib > 100;
  g.fillStyle = B.bg; g.fillRect(0, 0, cw, ch); g.clearRect(ix, iy, iw, ih);
  // 四角的定位角
  g.fillStyle = B.accent; const L = 34, T = 4;
  for (const r of [[ix - T, iy - T, L + T, T], [ix - T, iy - T, T, L + T], [ir - L, iy - T, L + T, T], [ir, iy - T, T, L + T], [ix - T, ib, L + T, T], [ix - T, ib - L, T, L + T], [ir - L, ib, L + T, T], [ir, ib - L, T, L + T]]) g.fillRect(...r);
  const mark = (x, y, s) => { g.fillStyle = B.accent; g.beginPath(); g.roundRect(x, y, s, s, s * 0.14); g.fill(); if (!WM_MARK) return; g.save(); g.translate(x + s * 0.16, y + s * 0.17); g.scale(s * 0.68 / 720, s * 0.68 / 720); g.translate(-280 - 57, -290); g.fillStyle = B.bg; g.fill(new Path2D(WM_MARK), 'evenodd'); g.restore(); };
  g.textBaseline = 'middle';
  // 右上角的小标
  { const h = 44, pad = 10; g.font = `700 20px ${cjk}`; const wn = g.measureText(B.name).width; g.font = `700 12px ${mono}`; const wc = info.no ? g.measureText('CASE').width + 8 + 26 : 0;
    const w = pad + 28 + 9 + wn + (wc ? 10 + wc : 0) + pad, x = ir - w - 22, y = iy + 22;
    g.fillStyle = 'rgba(10,10,10,.62)'; g.fillRect(x, y, w, h); mark(x + pad, y + 8, 28);
    g.fillStyle = '#fff'; g.font = `700 20px ${cjk}`; g.textAlign = 'left'; g.fillText(B.name, x + pad + 37, y + h / 2 + 1);
    if (info.no) { let cx = x + pad + 37 + wn + 10; g.font = `700 12px ${mono}`; g.fillStyle = B.accent; g.fillText('CASE', cx, y + h / 2 + 1); cx += g.measureText('CASE').width + 8; g.fillStyle = B.bg; g.fillRect(cx, y + 12, 26, 20); g.fillStyle = B.accent; g.textAlign = 'center'; g.fillText(info.no, cx + 13, y + h / 2 + 1); g.textAlign = 'left'; } }
  // 底栏：左边标识 + 编号 + 标题，右边网址 + 技术标签；竖版放两行
  const fit = (text, max) => { let s = text; while (s.length > 1 && g.measureText(s).width > max) s = s.slice(0, -2) + '…'; return s; };
  const title = info.title + (info.sub ? ' · ' + info.sub : ''), tagLine = info.tags.join('  ·  ');
  if (!tall) {
    const y = ib + (ch - ib) / 2 + 2; let x = ix;
    mark(x, y - 18, 36); x += 48; g.fillStyle = '#fff'; g.font = `700 24px ${cjk}`; g.fillText(B.name, x, y + 1); x += g.measureText(B.name).width + 22;
    if (info.no) { g.save(); g.translate(x, y); g.transform(1, 0, -0.28, 1, 0, 0); g.fillStyle = B.accent; g.fillRect(-3, -17, 6, 34); g.restore(); x += 22; g.font = `700 20px ${mono}`; g.fillStyle = B.accent; const t2 = 'CASE ' + info.no; g.fillText(t2, x, y + 1); x += g.measureText(t2).width + 36; }
    g.textAlign = 'right'; g.font = `700 15px ${mono}`; g.fillStyle = B.accent; g.fillText(B.site, ir, y - 9); const ws = g.measureText(tagLine).width;
    g.font = `500 14px ${mono}`; g.fillStyle = '#8a8a8a'; g.fillText(fit(tagLine, iw * 0.4), ir, y + 11);
    g.textAlign = 'left'; g.fillStyle = '#fff'; g.font = `500 22px ${cjk}`; g.fillText(fit(title, ir - x - Math.min(ws, iw * 0.4) - 40), x, y + 1);
  } else {
    const y1 = ib + 46, y2 = ib + 98; let x = ix;
    mark(x, y1 - 20, 40); x += 54; g.fillStyle = '#fff'; g.font = `700 28px ${cjk}`; g.fillText(B.name, x, y1 + 1); x += g.measureText(B.name).width + 22;
    if (info.no) { g.save(); g.translate(x, y1); g.transform(1, 0, -0.28, 1, 0, 0); g.fillStyle = B.accent; g.fillRect(-3, -18, 6, 36); g.restore(); x += 22; g.font = `700 22px ${mono}`; g.fillStyle = B.accent; g.fillText('CASE ' + info.no, x, y1 + 1); }
    g.textAlign = 'right'; g.font = `700 18px ${mono}`; g.fillStyle = B.accent; g.fillText(B.site, ir, y1 + 1);
    g.font = `500 15px ${mono}`; g.fillStyle = '#8a8a8a'; const tg = fit(tagLine, iw * 0.42); g.fillText(tg, ir, y2 + 1); const wt = g.measureText(tg).width;
    g.textAlign = 'left'; g.fillStyle = '#fff'; g.font = `500 26px ${cjk}`; g.fillText(fit(title, iw - wt - 30), ix, y2 + 1);
  }
  return new Promise((res, rej) => c.toBlob(async b => b ? res(Array.from(new Uint8Array(await b.arrayBuffer()))) : rej('水印图没画出来'), 'image/png'));
}
const wm = { scan: null, busy: false, cur: '', curWhat: '', errs: {}, info: {}, at: 3 };
function renderWm() {
  const s = wm.scan; if (!s || !$('#wm').classList.contains('on')) return;
  const todo = s.items.filter(i => !i.done).length, note = $('#wNote');
  note.hidden = !!s.ffmpeg; note.textContent = s.ffmpeg ? '' : '这台电脑上没找到 ffmpeg，加水印要用它。装好（或放到 根目录/_tools 里）后再点同步。';
  const webName = n => /^case\d{2,}_[a-z0-9]+(-[a-z0-9]+){0,3}_(16x9|9x16|3x4|4x3)(_v\d+)?$/.test(n);
  $('#wList').innerHTML = s.items.length ? `<div class="ann-sec">${s.items.map(i => {
    const big = i.done && i.size > 30 * 1048576, busy = wm.cur === i.rel;
    const st = busy ? (wm.curWhat || '正在生成…') : wm.errs[i.rel] ? '没成功' : i.done ? '已同步' : '待生成';
    const notes = [wm.errs[i.rel] || '无水印/' + i.rel];
    if (i.done) { notes.push(fmtMB(i.size) + (big ? '，超过官网的 30 MB' : '')); if (!i.poster) notes.push('还没有封面'); }
    if (!webName(i.name)) notes.push('文件名不是官网要的写法，不会进上传包');
    const acts = i.done && !wm.busy ? `${big ? `<button type="button" class="linkish" data-wm-shrink="${esc(i.rel)}">压到 30 MB 以内</button>` : ''}<button type="button" class="linkish" data-wm-poster="${esc(i.rel)}">${i.poster ? '换封面' : '生成封面'}</button>` : '';
    return `<div class="wm-row${busy ? ' busy' : ''}"><span>${esc(i.name)}</span><span class="st">${st}${wm.info[i.rel] ? ' · ' + wm.info[i.rel] : ''}</span><span class="sub">${esc(notes.join(' · '))}</span>${acts ? `<span class="sub acts">${acts}</span>` : ''}</div>`; }).join('')}</div>
    <label class="wm-at">封面取视频的第 <input class="inp mono" id="wAt" type="number" min="0" step="0.5" value="${wm.at}"> 秒</label>`
    : '<div class="set-info">「无水印」文件夹里还没有视频。把成片（mp4 / mov）放进去，再点刷新或同步。</div>';
  const go = $('#wGo'); go.disabled = wm.busy || !s.ffmpeg || !todo; go.textContent = wm.busy ? '正在同步…' : todo ? `同步水印版（${todo} 个）` : '都同步好了';
}
async function wmSync({ auto = false } = {}) {
  if (wm.busy || !root) return;
  wm.busy = true;
  try {
    wm.scan = await invoke('wm_scan', { root, create: !auto });
    const s = wm.scan, list = s.items.filter(i => !i.done && !(auto && wm.errs[i.rel]));
    if (!s.ffmpeg || !list.length) return;
    if (auto) toast(`正在后台给 ${list.length} 个案例视频加水印…`);
    let n = 0, bad = 0;
    for (const it of list) {
      wm.cur = it.rel; delete wm.errs[it.rel]; renderWm();
      try {
        const [w, h] = await invoke('wm_probe', { root, src: it.src });
        const key = wmPick(w, h), t = WM_TPL[key]; wm.info[it.rel] = key;
        const overlay = await wmDraw(t, wmInfo(it));
        await invoke('wm_render', { job: { root, rel: it.rel, overlay, ...t } });
        it.done = true; n++;
      } catch (e) { wm.errs[it.rel] = String(e); bad++; }
    }
    wm.cur = '';
    toast(`水印版已同步 ${n} 个` + (bad ? `，${bad} 个没成功（点「案例视频」看原因）` : ''));
  } catch (e) { if (!auto) toast('同步失败：' + e); }
  finally { wm.busy = false; wm.cur = ''; renderWm(); }
}
// 每次扫描后顺带看一眼：有新放进来的就后台生成（设置里可以关）
function wmAuto() { if (cfg.autoWm && root && !wm.busy) wmSync({ auto: true }); }
async function openWm() {
  if (!root) { pickRoot(); return; }
  closeAll(); $('#scrim').classList.add('on'); $('#wm').classList.add('on');
  try { if (!wm.busy) wm.scan = await invoke('wm_scan', { root, create: true }); } catch (e) { toast('读不到案例视频文件夹：' + e); }
  renderWm();
}
$('#openWm').addEventListener('click', openWm);
$('#wGo').addEventListener('click', () => wmSync());
$('#wList').addEventListener('change', e => { if (e.target.id === 'wAt') wm.at = Math.max(0, Number(e.target.value) || 0); });
$('#wList').addEventListener('click', async e => {
  const d = e.target.dataset || {}, rel = d.wmShrink || d.wmPoster; if (!rel || wm.busy) return;
  wm.busy = true; wm.cur = rel; wm.curWhat = d.wmShrink ? '正在压缩…' : '正在取封面…'; delete wm.errs[rel]; renderWm();
  try {
    if (d.wmShrink) { const size = await invoke('wm_shrink', { root, rel }); await invoke('wm_poster', { root, rel, at: wm.at }).catch(() => {}); toast(`压好了，现在 ${fmtMB(size)}`); }
    else { await invoke('wm_poster', { root, rel, at: wm.at }); toast(`封面已换成第 ${wm.at} 秒的画面`); }
  } catch (er) { wm.errs[rel] = String(er); }
  wm.busy = false; wm.cur = ''; wm.curWhat = '';
  try { wm.scan = await invoke('wm_scan', { root, create: false }); } catch (er) {}
  renderWm();
});
$('#wOpenSrc').addEventListener('click', () => wm.scan && invoke('reveal', { path: wm.scan.dir + '/无水印' }).catch(e => toast(String(e))));
$('#wOpenOut').addEventListener('click', () => wm.scan && invoke('reveal', { path: wm.scan.dir + '/水印版' }).catch(e => toast(String(e))));

function closeAll(keepScrim) {
  $('#modal').classList.remove('on'); $('#drawer').classList.remove('on'); $('#ann').classList.remove('on'); $('#imp').classList.remove('on'); $('#set').classList.remove('on'); $('#wm').classList.remove('on');
  closeConfirm(); closePop();
  if (!keepScrim) { $('#scrim').classList.remove('on'); ui.openId = null; }
}
let toastT;
function toast(msg) { const t = $('#toast'); t.textContent = msg; t.classList.add('on'); clearTimeout(toastT); toastT = setTimeout(() => t.classList.remove('on'), Math.max(3200, msg.length * 90)); }

document.addEventListener('click', e => {
  if ($('#bpop').classList.contains('on') && !e.target.closest('#bpop,#bbar')) closePop();
  const t = e.target.closest('button,[data-close]');
  if (!t) { if (e.target.id === 'scrim') closeAll(); return; }
  if (t.dataset.pick) { toggleSel(t.dataset.pick, e.shiftKey); return; }
  if (t.dataset.bact) { bact(t.dataset.bact, t); return; }
  if (t.dataset.bst) { batchStatus(t.dataset.bst); return; }
  if (t.dataset.badd !== undefined) { batchTagAdd(t.dataset.badd || ($('#bTagIn') ? $('#bTagIn').value : '')); return; }
  if (t.dataset.brm !== undefined) { batchTagRm(t.dataset.brm); return; }
  if (t.dataset.bdest !== undefined) { askBatchMove(t.dataset.bdest); return; }
  if (t.dataset.bdestNew !== undefined) { const v = $('#bDestIn').value.trim(); if (v) askBatchMove(v); else $('#bDestIn').focus(); return; }
  if (t.dataset.ann !== undefined) { openAnn(); return; }
  if (t.dataset.ast) { annStatus = t.dataset.ast; renderAnnOpts(); return; }
  if (t.dataset.unignore !== undefined) { ignored.clear(); saveIgnored(); scan(); toast('已恢复所有忽略的文件夹'); return; }
  if (t.dataset.unmissing !== undefined) { const have = ui.live || new Set(); imports = imports.filter(x => have.has(x.path)); saveImports(); ui.missingN = 0; renderSide(); renderSetInfo(); toast('已取消登记读不到的工程，文件没有动'); return; }
  if (t.id === 'openImport' || t.dataset.import !== undefined) { pickImport(); return; }
  if (t.id === 'openSet') { openSet(); return; }
  if (t.dataset.themeId !== undefined) { setTheme(t.dataset.themeId); return; }
  if (t.dataset.themeMode) { setThemeMode(t.dataset.themeMode); return; }
  if (t.id === 'sThemeDir') { openThemesDir(); return; }
  if (t.dataset.tpl !== undefined) { setTpl(t.dataset.tpl); return; }
  if (t.dataset.thumb) { setThumb(t.dataset.thumb); return; }
  if (t.id === 'npTplDir') { invoke('open_templates_dir').then(() => toast('模板文件夹已打开：每个文件夹就是一个模板，里面有说明')).catch(e => toast('打不开模板文件夹：' + e)); return; }
  if (t.id === 'sThemeReload') { reloadThemes(); return; }
  if (t.id === 'sLogoPick') { pickLogo(); return; }
  if (t.id === 'sLogoReset') { resetLogo(); return; }
  if (t.dataset.amain) { setMainToe(t.dataset.amain); return; }
  if (t.dataset.st) { ui.status = t.dataset.st; render(); }
  else if (t.dataset.tag !== undefined) { ui.tag = ui.tag === t.dataset.tag ? '' : t.dataset.tag; render(); }
  else if (t.dataset.grp !== undefined) { ui.group = ui.group === t.dataset.grp ? null : t.dataset.grp; render(); }
  else if (t.id === 'clear' || t.dataset.clear !== undefined) { ui.status = '全部'; ui.tag = ''; ui.group = null; ui.q = ''; $('#q').value = ''; render(); }
  else if (t.dataset.open) onCardClick(t.dataset.open, e);
  else if (t.id === 'openNew' || t.dataset.new !== undefined) openNew();
  else if (t.dataset.pickRoot !== undefined) pickRoot();
  else if (t.dataset.rescan !== undefined) scan();
  else if (t.dataset.close !== undefined) closeAll();
  else if (t.dataset.dst) { const p = cur(); if (!p) return; p.status = t.dataset.dst; renderDetailBits(p); render(); saveMeta(p); }
  else if (t.dataset.rm) { const p = cur(); if (!p) return; p.tags = p.tags.filter(x => x !== t.dataset.rm); renderDetailBits(p); render(); saveMeta(p); }
});
$('#q').addEventListener('input', e => { ui.q = e.target.value; renderSide(); renderView(); syncSel(); });
document.addEventListener('keydown', e => {
  if (e.key === 'Escape') {
    if ($('#cfm').classList.contains('on')) closeConfirm();
    else if ($('#bpop').classList.contains('on')) closePop();
    else if ($('#scrim').classList.contains('on')) closeAll();
    else if (ui.sel.size) clearSel();
  }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a' && !(e.target.closest && e.target.closest('input,textarea')) && !$('#scrim').classList.contains('on') && ui.visible.length) { e.preventDefault(); selectAll(); }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'n') { e.preventDefault(); openNew(); }
  if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'r')) { e.preventDefault(); scan(); }
});
// 从 TD 切回来时重新扫一遍，刚保存的缩略图就能出现
window.addEventListener('focus', () => { if (root && cfg.autoRescan) scan({ quiet: true }); });


syncRoot();
loadLogo();
loadThemes();
render();
scan();
