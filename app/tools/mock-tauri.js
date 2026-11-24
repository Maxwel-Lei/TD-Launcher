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
    mk('2026-10-28_音频可视化', '音频可视化', { name: '音频可视化', tags: ['音频可视化', '演出'], status: '已交付', client: 'Livehouse', created: '2026-10-28' }),
    mk('GLSL练习', 'glsl', { name: 'GLSL 噪声练习', tags: ['GLSL'], status: '搁置' }),
    mk('老工程_没有标注', 'old_project', null),
    mk('客户A_海报', 'poster', null, true, '01_商单/客户A'),
    mk('粒子教程', 'tutorial', null, true, '02_案例工程', { confidence: 'maybe' }),
    mk('Fiction plant', 'Fiction plant', null, true, '01_商单', { toeBases: ['Fiction Plant-Remix', 'Fiction Plant-Remix-recordingEdition'] })
  ];
  const calls = (window.__mockCalls = []);
  window.__TAURI__ = {
    core: {
      convertFileSrc: p => p,
      invoke: async (cmd, args) => {
        calls.push([cmd, args]);
        if (cmd === 'scan_projects') return JSON.parse(JSON.stringify(list));
        if (cmd === 'create_project') { const p = mk(args.folder, args.name, { name: args.name, tags: [], status: '进行中', created: args.created }, false); list.unshift(p); return { path: p.path, toe: null }; }
        if (cmd === 'delete_project') { list = list.filter(x => x.path !== args.path); return null; }
        if (cmd === 'save_meta') { const p = list.find(x => x.path === args.path); p.meta = Object.assign(p.meta || {}, args.meta); p.hasMeta = true; p.confidence = 'meta'; return null; }
        if (cmd === 'move_project') { const p = list.find(x => x.path === args.path); if (args.dest === '01_商单' && p.folder === 'GLSL练习') throw '文件被占用（可能正在 TD 里打开），没有移动'; p.group = args.dest; p.path = 'D:/TD Projects/' + (args.dest ? args.dest + '/' : '') + p.folder; return p.path; }
        return null;
      }
    },
    dialog: { open: async () => 'D:/TD Projects' }
  };
  try { localStorage.setItem('root', 'D:/TD Projects'); } catch (e) {}
})();
