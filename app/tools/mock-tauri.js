// 浏览器预览用的假后端：模拟 window.__TAURI__，让 src/app.js 不开软件也能跑
// 只在 tools/preview.html 里加载，打包的软件不会用到它
(() => {
  const now = Math.floor(Date.now() / 1000);
  const mk = (folder, name, meta, toe = true, group = '', extra = {}) => ({
    path: 'D:/TD Projects/' + (group ? group + '/' : '') + folder, folder, group, name, toe: toe ? `D:/TD Projects/${folder}/${name}.toe` : null,
    toeBases: toe ? [name] : [], confidence: meta ? 'meta' : 'likely', earliest: now - 9e6,
    thumb: null, thumbMtime: 0, modified: now - Math.floor(Math.random() * 9e5), hasMeta: !!meta, meta: meta || null, ...extra
  });
  let list = [
    mk('2026-11-12_粒子雨', '粒子雨', { name: '粒子雨', tags: ['粒子', 'GPU'], status: '进行中', note: '200 万粒子掉帧', client: '', created: '2026-11-12' }),
    mk('2026-10-28_音频可视化', '音频可视化', { name: '音频可视化', tags: ['音频可视化', '演出'], status: '已完成', client: 'Livehouse', created: '2026-10-28' }),
    mk('GLSL练习', 'glsl', { name: 'GLSL 噪声练习', tags: ['GLSL'], status: '搁置' }),
    mk('老工程_没有标注', 'old_project', null, true, '', { thumb: '../app-icon.svg', thumbAuto: true, thumbMtime: 1 }),
    mk('客户A_海报', 'poster', null, true, '01_商单/客户A'),
    mk('粒子教程', 'tutorial', null, true, '02_案例工程', { confidence: 'maybe' }),
    mk('Fiction plant', 'Fiction plant', null, true, '01_商单', { toeBases: ['Fiction Plant-Remix', 'Fiction Plant-Remix-recordingEdition'] })
  ];
  // 根目录以外的工程，给"导入"用：选 E:/旧硬盘 是批量，选其中一个是单个
  const ext = (folder, name, meta, grp = '旧硬盘') => Object.assign(mk(folder, name, meta, true, grp), { path: 'E:/旧硬盘/' + folder, toe: `E:/旧硬盘/${folder}/${name}.toe`, external: true });
  const outside = [ext('毕设_云南文物', 'relic', null), ext('2025_音乐节VJ', 'vj', { name: '音乐节 VJ', tags: ['演出'], status: '已交付', created: '2025-08-02' }), ext('测试用', 'test', null, '02_案例工程')];
  const tpls = [{ name: '演出', hasToe: true, entries: ['灯光/', '素材/', '音频/', '清单.md'] }, { name: '接单', hasToe: false, entries: ['01_需求/', '02_素材/', '03_工程/', '04_交付/', '报价单.md'] }];
  const calls = (window.__mockCalls = []);
  window.__TAURI__ = {
    core: {
      convertFileSrc: p => (String(p).startsWith('blob:') ? p + '#' : p),
      invoke: async (cmd, args) => {
        calls.push([cmd, args]);
        if (cmd === 'scan_projects') return JSON.parse(JSON.stringify(list.concat(outside.filter(x => (args.extra || []).includes(x.path)).map(x => Object.assign({}, x, { group: '外部导入' })))));
        if (cmd === 'inspect_import') {
          if (args.path === 'E:/旧硬盘') return { insideRoot: false, projects: JSON.parse(JSON.stringify(outside)) };
          const one = outside.find(x => x.path === args.path);
          return { insideRoot: args.path.startsWith('D:/TD Projects'), projects: one ? [JSON.parse(JSON.stringify(one))] : [] };
        }
        if (cmd === 'read_image') return Array.from(new Uint8Array(await (await fetch(String(args.path).startsWith('blob:') ? args.path : (window.__mockLogo || '../app-icon.svg'))).arrayBuffer()));
        if (cmd === 'save_custom_logo') { window.__saved = args; window.__logoUrl = URL.createObjectURL(new Blob([new Uint8Array(args.logo)], { type: 'image/png' })); return 'blob'; }
        if (cmd === 'clear_custom_logo') { window.__logoUrl = null; return null; }
        if (cmd === 'custom_logo') return window.__logoUrl ? [window.__logoUrl, 1] : null;
        if (cmd === 'set_thumb') { const p = list.concat(outside).find(x => x.path === args.path); p.meta = Object.assign(p.meta || {}, { thumbFile: args.file === null ? '-' : args.file ? 'picked.png' : '' }); p.hasMeta = true; if (args.file === null) { p.thumb = null; p.thumbAuto = false; } else if (args.file) { p.thumb = '../app-icon.svg'; p.thumbAuto = false; } else { p.thumb = '../app-icon.svg'; p.thumbAuto = true; } return null; }
        if (cmd === 'list_templates') return tpls;
        if (cmd === 'path_kind') return /nope/.test(args.path) ? '' : /\.tox$/.test(args.path) ? 'file' : 'dir';
        if (cmd === 'check_case') return [{ label: '工程文件在第一层', ok: true, hint: '' }, { label: '有演示视频', ok: false, hint: '把成片放进 demo 文件夹' }];
        if (cmd === 'wm_scan') { window.__wm = window.__wm || [{ rel: 'CASE01_手势寻宝_握拳收集.mov', name: 'CASE01_手势寻宝_握拳收集', src: 'x', out: 'y', done: false }, { rel: '竖版/CASE02_晕染.mp4', name: 'CASE02_晕染', src: 'x2', out: 'y2', done: true }]; return { dir: args.root + '/案例视频', exists: true, ffmpeg: 'ffmpeg', items: window.__wm.map(x => ({ ...x })) }; }
        if (cmd === 'wm_probe') return args.src === 'x' ? [1920, 1080] : [1080, 1920];
        if (cmd === 'wm_render') { window.__wmJobs = (window.__wmJobs || []).concat([args.job]); window.__wm.find(x => x.rel === args.job.rel).done = true; return 'ok'; }
        if (cmd === 'open_templates_dir') return 'C:/Users/me/AppData/Roaming/com.tdxuezhang.launcher/templates';
        if (cmd === 'save_as_template') { if (tpls.some(t => t.name === args.name)) throw `已经有叫「${args.name}」的模板了`; tpls.push({ name: args.name, hasToe: true, entries: ['assets/', 'export/'] }); return null; }
        if (cmd === 'list_user_themes') return window.__userThemes || [];
        if (cmd === 'open_themes_dir') return 'C:/Users/me/AppData/Roaming/com.tdxuezhang.launcher/themes';
        if (cmd === 'set_folder_icon') { const p = list.concat(outside).find(x => x.path === args.path); if (p.folder === 'GLSL练习' && args.on) throw '这个文件夹已经有自己的图标设置（desktop.ini），没有改动'; p.folderIcon = args.on; return null; }
        if (cmd === 'create_project') { window.__created = args; const p = mk(args.folder, args.name, { name: args.name, tags: [], status: '进行中', created: args.created }, false); list.unshift(p); return { path: p.path, toe: null }; }
        if (cmd === 'delete_project') { list = list.filter(x => x.path !== args.path); return null; }
        if (cmd === 'save_meta') { const p = list.concat(outside).find(x => x.path === args.path); p.meta = Object.assign(p.meta || {}, args.meta); p.hasMeta = true; p.confidence = 'meta'; return null; }
        if (cmd === 'move_project') { let p = list.find(x => x.path === args.path); if (!p) { const i = outside.findIndex(x => x.path === args.path); p = outside.splice(i, 1)[0]; p.external = false; list.push(p); } if (args.dest === '01_商单' && p.folder === 'GLSL练习') throw '文件被占用（可能正在 TD 里打开），没有移动'; p.group = args.dest; p.path = 'D:/TD Projects/' + (args.dest ? args.dest + '/' : '') + p.folder; return p.path; }
        return null;
      }
    },
    dialog: { open: async o => (o && o.filters ? 'C:/logo/my-logo.svg' : o && o.multiple ? (window.__mockPick || ['E:/旧硬盘']) : 'D:/TD Projects') }
  };
  try { localStorage.setItem('root', 'D:/TD Projects'); } catch (e) {}
})();
