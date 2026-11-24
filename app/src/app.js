/* TD 工程启动器 · 前端
   界面沿用 demo/index.html；数据来自 Rust 后端（src-tauri/src/lib.rs）：
     scan_projects   扫描根目录下的工程
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

const STATUSES = ['进行中', '已交付', '搁置'];
const ICON = { '全部': 's0', '进行中': 's1', '已交付': 's2', '搁置': 's3', '未标注': 's4' };
const ui = { status: '全部', tag: '', group: null, q: '', openId: null, scanning: false, error: '' };
const topGroup = p => p.group ? p.group.split('/')[0] : '';
let root = localStorage.getItem('root') || '';
let projects = [];

const $ = s => document.querySelector(s);
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const today = () => { const d = new Date(), p = n => String(n).padStart(2, '0'); return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`; };
const fmtTime = s => { if (!s) return ''; const d = new Date(s * 1000), p = n => String(n).padStart(2, '0'); return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`; };
const shortPath = p => p.replace(/\\/g, '/');

/* ================= 融球点阵（与 Demo 相同） ================= */
const WAIST = 0.42;
const fx = new Map();
const reduceMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;
function seeded(id) { let s = (Math.abs(id) % 100000) * 9301 + 49297; return () => (s = (s * 9301 + 49297) % 233280) / 233280; }
function hash(str) { let h = 0; for (const ch of str) h = (h * 31 + ch.charCodeAt(0)) % 99991; return h; }
function registerFx(canvas, cfg) { if (!canvas) return; fx.set(canvas, cfg); drawFx(canvas, cfg, T); }
function sizeCanvas(c) {
  const dpr = Math.min(window.devicePixelRatio || 1, 2), w = c.clientWidth, h = c.clientHeight;
  if (!w || !h) return false;
  if (c.width !== Math.round(w * dpr) || c.height !== Math.round(h * dpr)) { c.width = Math.round(w * dpr); c.height = Math.round(h * dpr); }
  c._w = w; c._h = h; c._dpr = dpr; return true;
}
function buildLayer(L, w, h) {
  const S = L.S, R = S * (L.ratio || 0.28), rnd = seeded(L.seed || 1);
  const cols = Math.ceil(w / S) + 1, rows = Math.ceil(h / S) + 1;
  const ox = (w - (cols - 1) * S) / 2, oy = (h - (rows - 1) * S) / 2;
  const at = {}, nodes = [], links = [];
  for (let j = 0; j < rows; j++) for (let i = 0; i < cols; i++) {
    const x = ox + i * S, y = oy + j * S;
    const inside = L.clusters.some(c => ((x - c.x * w) / c.rx) ** 2 + ((y - c.y * h) / c.ry) ** 2 <= 1);
    if (inside && rnd() < (L.keep ?? 0.85)) { const n = { x, y }; nodes.push(n); at[i + ',' + j] = n; }
  }
  for (const key in at) {
    const [i, j] = key.split(',').map(Number), a = at[key];
    for (const b of [at[(i + 1) + ',' + j], at[i + ',' + (j + 1)]]) {
      if (b && rnd() < (L.linkP ?? 0.6)) links.push({ a, b, sp: 0.35 + rnd() * 0.5, ph: rnd() * 6.283, bias: rnd() * 0.8 - 0.3 });
    }
  }
  return { R, nodes, links };
}
const TAU = Math.PI * 2;
function arcThrough(ctx, c, r, p1, p2, m) {
  const a1 = Math.atan2(p1[1] - c[1], p1[0] - c[0]), a2 = Math.atan2(p2[1] - c[1], p2[0] - c[0]), am = Math.atan2(m[1] - c[1], m[0] - c[0]);
  const sweep = ((a2 - a1) % TAU + TAU) % TAU, sm = ((am - a1) % TAU + TAU) % TAU;
  ctx.arc(c[0], c[1], r, a1, a2, !(sm < sweep));
}
function bridge(ctx, A, B, R, wst) {
  const dx = B.x - A.x, dy = B.y - A.y, d = Math.hypot(dx, dy), ux = dx / d, uy = dy / d, nx = -uy, ny = ux, half = d / 2;
  const f = (wst * wst + half * half - R * R) / (2 * (R - wst));
  if (!(f > 0.01)) return;
  const mx = A.x + ux * half, my = A.y + uy * half, k = R / (R + f);
  const Ct = [mx + nx * (wst + f), my + ny * (wst + f)], Cb = [mx - nx * (wst + f), my - ny * (wst + f)];
  const tp = (P, C) => [P.x + (C[0] - P.x) * k, P.y + (C[1] - P.y) * k];
  const TAt = tp(A, Ct), TBt = tp(B, Ct), TAb = tp(A, Cb), TBb = tp(B, Cb);
  ctx.moveTo(TAt[0], TAt[1]);
  arcThrough(ctx, Ct, f, TAt, TBt, [mx + nx * wst, my + ny * wst]);
  ctx.lineTo(TBb[0], TBb[1]);
  arcThrough(ctx, Cb, f, TBb, TAb, [mx - nx * wst, my - ny * wst]);
  ctx.closePath();
}
function drawFx(c, cfg, t) {
  if (!sizeCanvas(c)) return;
  const ctx = c.getContext('2d'), w = c._w, h = c._h;
  ctx.setTransform(c._dpr, 0, 0, c._dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  for (const L of cfg.layers) {
    const key = w + 'x' + h;
    if (L._key !== key) { L._g = buildLayer(L, w, h); L._key = key; }
    const { R, nodes, links } = L._g;
    ctx.fillStyle = L.color;
    ctx.beginPath();
    for (const n of nodes) { ctx.moveTo(n.x + R, n.y); ctx.arc(n.x, n.y, R, 0, TAU); }
    ctx.fill();
    ctx.beginPath();
    for (const l of links) {
      let s = Math.sin(t * l.sp + l.ph) * 1.6 + l.bias;
      if (s <= 0) continue;
      s = Math.min(1, s); s = s * s * (3 - 2 * s);
      const wst = s * R * WAIST;
      if (wst > 0.3) bridge(ctx, l.a, l.b, R, wst);
    }
    ctx.fill();
  }
}
let T = 0, last = performance.now();
function loop(now) {
  const dt = Math.min(0.05, (now - last) / 1000); last = now;
  if (!reduceMotion && !document.hidden) T += dt * 0.9;
  for (const [c, cfg] of fx) {
    if (!c.isConnected) { fx.delete(c); continue; }
    if (c.offsetParent === null && c.id !== 'bgfx') continue;
    drawFx(c, cfg, T);
  }
  requestAnimationFrame(loop);
}
requestAnimationFrame(loop);

/* ================= 数据 ================= */
function normalize(raw) {
  const m = raw.meta || {};
  const fromFolder = (raw.folder.match(/^(\d{4}-\d{2}-\d{2})/) || [])[1];
  return {
    id: raw.path,
    path: raw.path,
    folder: raw.folder,
    group: raw.group || '',
    toe: raw.toe,
    thumb: raw.thumb,
    thumbMtime: raw.thumbMtime,
    modified: raw.modified,
    hasMeta: raw.hasMeta,
    confidence: raw.confidence || 'maybe',
    toeBases: raw.toeBases || [],
    mainToe: m.mainToe || '',
    name: m.name || raw.name,
    tags: Array.isArray(m.tags) ? m.tags : [],
    status: STATUSES.includes(m.status) ? m.status : '未标注',
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
    const list = await invoke('scan_projects', { root });
    if (seq !== scanSeq) return;
    loadIgnored();
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
  $('#ignRow').innerHTML = ui.ignoredN ? `<div class="ign">已忽略 ${ui.ignoredN} 个文件夹 · <button type="button" data-unignore>全部恢复</button></div>` : '';
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
    <div class="thumb${th ? '' : ' empty'}">${th ? `<img src="${th}" alt="" loading="lazy">` : `<canvas data-ph="${hash(p.id)}" aria-hidden="true"></canvas><div class="hint"><b>还没有缩略图</b>${p.toe ? '用 TD 打开并保存一次即可生成' : '这个文件夹里还没有 .toe'}</div>`}</div>
    <div class="info">
      <div class="r1"><span class="name" title="${esc(p.name)}">${esc(p.name)}</span><span class="date">${esc(p.created)}</span></div>
      <div class="r2"><i class="ico ${ICON[p.status]}"></i><span class="st">${p.status}</span><span class="sep">/</span><span class="tg">${p.tags.map(esc).join(' · ') || '无标签'}</span></div>
    </div></button></div>`;
}

function emptyHTML(title, text, actions) {
  return `<div class="empty-state">
    <canvas id="emptyfx" aria-hidden="true"></canvas>
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
      <div class="grid">${[0, 1, 2, 3, 4, 5].map(i => `<div class="card cut skel"><div class="thumb empty"><canvas data-sk="${i}" aria-hidden="true"></canvas></div><div class="info" style="gap:10px;padding-bottom:18px"><div class="bar1"></div><div class="bar2"></div></div></div>`).join('')}</div>`;
    v.querySelectorAll('canvas[data-sk]').forEach(c => { const i = +c.dataset.sk; registerFx(c, { layers: [{ S: 26, color: '#C9C9C3', seed: 10 + i, keep: 0.8, linkP: 0.7, clusters: [{ x: 0.5, y: 0.5, rx: 120, ry: 60 }] }] }); });
    return;
  } else if (!projects.length) {
    v.innerHTML = emptyHTML('这里还没有工程', '新建一个工程，或者把工程根目录换成你已有 TD 工程所在的文件夹。',
      '<button class="btn cut" type="button" data-new>新建第一个工程</button><button class="btn ghost cut cut-line" type="button" data-pick-root>换一个根目录</button>');
  } else {
    const q = ui.q.trim().toLowerCase();
    const list = projects.filter(p => (ui.status === '全部' || p.status === ui.status) && (!ui.tag || p.tags.includes(ui.tag)) &&
      (ui.group === null || topGroup(p) === ui.group) &&
      (!q || p.name.toLowerCase().includes(q) || p.folder.toLowerCase().includes(q) || p.group.toLowerCase().includes(q)));
    ui.visible = list.map(p => p.id);
    const un = projects.filter(p => !p.hasMeta).length;
    const annbar = un && (ui.status === '全部' || ui.status === '未标注') ? `<div class="annbar cut"><i class="ico s4"></i><span>有 <b>${un}</b> 个工程还没有 project.json，可以一次补上标注</span><button class="btn text" type="button" data-ann>识别并补标注</button></div>` : '';
    let head = ui.status === '全部' ? '全部工程' : ui.status; if (ui.tag) head += ' · ' + ui.tag; if (ui.group !== null) head += ' · ' + (ui.group || '根目录');
    v.innerHTML = `<div class="main-h"><div><span class="label">Projects</span><h1>${esc(head)}</h1></div>
      <div class="count">共 <span class="num">${list.length}</span> 个工程 · 最近修改在前 · 按住 Ctrl 点卡片可多选</div></div>
      ${annbar}${list.length ? `<div class="grid">${list.map(cardHTML).join('')}</div>` :
        `<div class="nomatch"><canvas id="nmfx" aria-hidden="true"></canvas><div style="font-weight:500">没有符合条件的工程</div><button class="btn ghost cut cut-line" type="button" data-clear>清除筛选</button></div>`}`;
    v.querySelectorAll('canvas[data-ph]').forEach(c => {
      registerFx(c, { layers: [{ S: 26, color: '#BDBDB6', seed: +c.dataset.ph, keep: 0.8, linkP: 0.65, clusters: [{ x: 0.66, y: 0.4, rx: 95, ry: 55 }] }] });
    });
    if ($('#nmfx')) registerFx($('#nmfx'), { layers: [{ S: 24, color: '#C4C4BE', seed: 2, keep: 0.85, linkP: 0.7, clusters: [{ x: 0.5, y: 0.5, rx: 80, ry: 40 }] }] });
    return;
  }
  registerFx($('#emptyfx'), { layers: [
    { S: 34, color: '#C4C4BE', seed: 5, keep: 0.85, linkP: 0.65, clusters: [{ x: 0.4, y: 0.56, rx: 125, ry: 80 }] },
    { S: 34, color: '#111111', seed: 9, keep: 1, linkP: 0.9, clusters: [{ x: 0.78, y: 0.28, rx: 50, ry: 42 }] }
  ] });
}
function render() { renderSide(); renderView(); syncSel(); }

/* ================= 新建工程 ================= */
const BAD = /[:?*\/\\<>|"]/g;
function folderName() {
  const base = $('#npName').value.trim().replace(/[. ]+$/, '');
  return { base, folder: ($('#npDate').checked ? today() + '_' : '') + base };
}
function updateTree() {
  const { base, folder } = folderName(), b = base || '未命名', f = folder || b;
  $('#npTree').innerHTML = [`<b>${esc(f || b)}/</b>`, `├─ ${esc(b)}.toe`, '├─ project.json   ← 标签、状态、备注', '├─ thumb.jpg      ← 保存时自动生成',
    '├─ assets/', '│  ├─ images/', '│  ├─ videos/', '│  ├─ audio/', '│  ├─ models/', '│  └─ fonts/', '├─ tox/', '├─ scripts/', '└─ export/'].join('\n');
}
$('#npName').addEventListener('input', e => {
  const raw = e.target.value, clean = raw.replace(BAD, '');
  if (clean !== raw) { const pos = Math.max(0, e.target.selectionStart - (raw.length - clean.length)); e.target.value = clean; e.target.setSelectionRange(pos, pos); $('#npWarn').hidden = false; }
  updateTree();
});
$('#npName').addEventListener('keydown', e => { if (e.key === 'Enter') $('#npCreate').click(); });
$('#npDate').addEventListener('change', e => { localStorage.setItem('datePrefix', e.target.checked ? '1' : '0'); updateTree(); });
$('#npCreate').addEventListener('click', async () => {
  if (!root) { toast('先选择工程根目录'); return; }
  const { base, folder } = folderName();
  if (!base) { toast('先给工程起个名字'); $('#npName').focus(); return; }
  const btn = $('#npCreate'); btn.disabled = true;
  try {
    const res = await invoke('create_project', { root, folder, name: base, created: today() });
    closeAll();
    await scan({ quiet: true });
    if (res.toe) {
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
  $('#npDate').checked = localStorage.getItem('datePrefix') !== '0';
  updateTree(); syncRoot();
  $('#scrim').classList.add('on'); $('#modal').classList.add('on');
  registerFx($('#mfx'), { layers: [{ S: 22, color: '#D2D2CC', seed: 4, keep: 0.85, linkP: 0.65, clusters: [{ x: 0.86, y: 0.3, rx: 110, ry: 70 }] }] });
  setTimeout(() => $('#npName').focus(), 60);
}

/* ================= 详情抽屉 ================= */
const cur = () => projects.find(x => x.id === ui.openId);
function refreshDetailHead(p) {
  const th = thumbSrc(p);
  $('#dThumb').innerHTML = th ? `<img src="${th}" alt="${esc(p.name)} 的缩略图">` : '<canvas id="dph" aria-hidden="true"></canvas>';
  $('#dThumb').style.background = th ? '#161616' : 'var(--bg1)';
  if (!th) registerFx($('#dph'), { layers: [{ S: 30, color: '#BDBDB6', seed: hash(p.id), keep: 0.85, linkP: 0.65, clusters: [{ x: 0.5, y: 0.5, rx: 150, ry: 80 }] }] });
  $('#dName').textContent = p.name;
  $('#dPath').textContent = shortPath(p.path);
  $('#dPath').title = p.group ? '分类：' + p.group : '';
  $('#dMeta').textContent = `创建于 ${p.created || '未知'} · ${th ? '缩略图更新于 ' + fmtTime(p.thumbMtime) : '还没有缩略图'}${p.toe ? '' : ' · 没有 .toe'}`;
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
  refreshDetailHead(p);
  $('#dClient').value = p.client; $('#dNote').value = p.note;
  renderDetailBits(p);
  $('#scrim').classList.add('on'); $('#drawer').classList.add('on');
  registerFx($('#dfx'), { layers: [{ S: 22, color: '#D2D2CC', seed: 8, keep: 0.85, linkP: 0.65, clusters: [{ x: 0.86, y: 0.4, rx: 100, ry: 60 }] }] });
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

// 删除单个工程：整个文件夹移到回收站
$('#dDel').addEventListener('click', () => {
  const p = cur(); if (!p) return;
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
ui.sel = new Set(); ui.anchor = null; ui.visible = []; ui.ignoredN = 0;
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
  const projRel = new Set(projects.map(p => p.group ? p.group + '/' + p.folder : p.folder));
  const out = new Set(['']);
  for (const p of projects) {
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
    confirmBox({
      label: 'Delete', title: `删除 ${sel.length} 个工程？`, yes: '移到回收站',
      html: `这些工程的整个文件夹会移到系统回收站，删错了可以从回收站还原。${listHTML(sel)}`,
      onYes: async () => report(await runBatch(sel, p => invoke('delete_project', { root, path: p.path })), n => `已把 ${n} 个工程移到回收站`)
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
    html: `移到 <b>${esc(dest || '根目录')}</b>，这个分类文件夹不存在会自动新建。<br>移动的是真实文件夹；目标里有同名文件夹、或工程正在 TD 里打开的会跳过。${listHTML(sel)}`,
    onYes: async () => report(await runBatch(sel, p => invoke('move_project', { root, path: p.path, dest })), n => `已移动 ${n} 个工程`)
  });
}

/* ================= 老工程补标注（企划 §6.2） ================= */
const groupTag = p => { const g = topGroup(p); return g ? (g.replace(/^\d+[\s_.\-]*/, '') || g) : ''; };
let annStatus = '已交付';
function openAnn() {
  const list = projects.filter(p => !p.hasMeta);
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
  registerFx($('#afx'), { layers: [{ S: 22, color: '#D2D2CC', seed: 12, keep: 0.85, linkP: 0.65, clusters: [{ x: 0.86, y: 0.35, rx: 120, ry: 70 }] }] });
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

/* ================= 通用 ================= */
function closeAll(keepScrim) {
  $('#modal').classList.remove('on'); $('#drawer').classList.remove('on'); $('#ann').classList.remove('on');
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
window.addEventListener('focus', () => { if (root) scan({ quiet: true }); });

registerFx($('#bgfx'), { layers: [
  { S: 40, color: '#D0D0CA', seed: 3, keep: 0.88, linkP: 0.6, clusters: [{ x: 0.9, y: 0.88, rx: 280, ry: 210 }, { x: 0.25, y: 1.02, rx: 300, ry: 130 }] },
  { S: 40, color: '#A6A69F', seed: 6, keep: 0.9, linkP: 0.75, clusters: [{ x: 0.96, y: 0.07, rx: 130, ry: 110 }] }
] });
new ResizeObserver(() => drawFx($('#bgfx'), fx.get($('#bgfx')), T)).observe($('#main'));

syncRoot();
render();
scan();
