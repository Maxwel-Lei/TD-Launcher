//! TD 工程启动器 · 后端
//! 只做"工程文件夹"这一层的事：扫描识别、新建、导入、读写 project.json、打开、在文件夹中显示、删除（回收站）、移动、文件夹图标。

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

/// 新建工程时创建的子文件夹（企划 §4）
const SUBDIRS: &[&str] = &[
    "assets/images",
    "assets/videos",
    "assets/audio",
    "assets/models",
    "assets/fonts",
    "tox",
    "scripts",
    "export",
];
/// Windows 不允许出现在文件名里的字符
const BAD_CHARS: &[char] = &[':', '?', '*', '/', '\\', '<', '>', '|', '"'];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    path: String,
    folder: String,
    /// 所在的分类文件夹（相对根目录），根目录下直接的工程为空
    group: String,
    name: String,
    /// 主 .toe（企划 §6.1）
    toe: Option<String>,
    /// 文件夹里不同名字的 .toe（已去掉版本号），多于一个时让用户选主文件
    toe_bases: Vec<String>,
    /// 识别把握：meta（有 project.json）/ likely（有 TD 或模板结构）/ maybe（只有零散 .toe）
    confidence: &'static str,
    /// 最早一份 .toe（含 Backup 里的）的修改时间，用来推测老工程的创建日期
    earliest: u64,
    thumb: Option<String>,
    thumb_mtime: u64,
    /// 预览图不是 thumb.jpg 也不是用户指定的，而是软件在文件夹里自动找的一张现成的图
    thumb_auto: bool,
    modified: u64,
    has_meta: bool,
    meta: Value,
    /// 从根目录以外导入的工程：原地登记，文件留在原处（企划 §3）
    external: bool,
    /// 文件夹已经换上了启动器的图标
    folder_icon: bool,
}

/// 导入的工程在侧栏"文件夹"里归到这一组
const EXT_GROUP: &str = "外部导入";
/// 文件夹图标：图标文件放在工程文件夹里，desktop.ini 用相对路径指向它，挪硬盘也不会丢
const ICON_FILE: &str = ".td-folder.ico";
const ICON_INI: &str = "[.ShellClassInfo]\r\nIconResource=.td-folder.ico,0\r\n";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Created {
    path: String,
    toe: Option<String>,
}

fn mtime(p: &Path) -> u64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 去掉 TD 的版本号：粒子雨.12 → 粒子雨
fn strip_version(stem: &str) -> String {
    match stem.rsplit_once('.') {
        Some((head, tail)) if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) => head.to_string(),
        _ => stem.to_string(),
    }
}

fn read_meta(dir: &Path) -> Option<Value> {
    let s = fs::read_to_string(dir.join("project.json")).ok()?;
    serde_json::from_str::<Value>(s.trim_start_matches('\u{feff}')).ok()
}

/* ---------------- 识别规则（企划 §6.1） ---------------- */

/// 系统杂项：两个平台用同一份名单，第二项标注是哪个系统产生的。
/// Mac 版和 Windows 版都要跳过：拷到 U 盘 / 移动硬盘或打包 zip 时会互相带过去。
const SYSTEM_JUNK: &[(&str, &str)] = &[
    ("__macosx", "macOS"),        // Mac 打包 zip 时附带的影子文件夹
    (".ds_store", "macOS"),       // Finder 的显示设置
    (".trashes", "macOS"),        // 外接硬盘上的废纸篓
    (".spotlight-v100", "macOS"), // Spotlight 索引
    (".fseventsd", "macOS"),      // 文件变动记录
    ("$recycle.bin", "Windows"),  // 回收站
    ("system volume information", "Windows"),
    ("thumbs.db", "Windows"), // 资源管理器缩略图缓存
];

/// 返回这个名字属于哪个系统的杂项；`._` 开头的是 macOS 的 AppleDouble 影子文件
fn system_junk(name: &str) -> Option<&'static str> {
    let l = name.to_lowercase();
    if l.starts_with("._") {
        return Some("macOS");
    }
    SYSTEM_JUNK.iter().find(|(n, _)| *n == l).map(|(_, os)| *os)
}

/// 扫描时整个跳过的文件夹；top 表示它直接在根目录下
fn skip_dir(name: &str, _top: bool) -> bool {
    let l = name.to_lowercase();
    l.starts_with('.')
        || l.starts_with('_')
        || l == "backup"
        || l == "node_modules"
        || system_junk(name).is_some()
}

/// 算数的 .toe：排除 Mac 影子文件和崩溃自动存档
fn is_valid_toe(p: &Path) -> bool {
    let Some(name) = p.file_name().map(|n| n.to_string_lossy().to_lowercase()) else { return false };
    name.ends_with(".toe") && !name.starts_with("._") && !name.starts_with("crashautosave") && p.is_file()
}

struct Toe {
    path: PathBuf,
    base: String,
    mtime: u64,
}

/// 列出文件夹里的 .toe，并把同一个文件的不同版本归成一类。
/// 只有"去掉版本号后的名字"真的存在，或至少两个文件共用它时，才当版本号去掉，
/// 这样 StreamDiffusionTD_0.3.0.toe 这种名字里本来就带数字的不会被拆错。
fn toes_in(dir: &Path) -> Vec<Toe> {
    let Ok(rd) = fs::read_dir(dir) else { return vec![] };
    let files: Vec<(PathBuf, String)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_valid_toe(p))
        .map(|p| {
            let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
            (p, stem)
        })
        .collect();
    let stems: std::collections::HashSet<&str> = files.iter().map(|(_, s)| s.as_str()).collect();
    let mut shared: std::collections::HashMap<String, usize> = Default::default();
    for (_, s) in &files {
        *shared.entry(strip_version(s)).or_default() += 1;
    }
    files
        .iter()
        .map(|(p, stem)| {
            let stripped = strip_version(stem);
            let base = if stripped != *stem && (stems.contains(stripped.as_str()) || shared[&stripped] >= 2) { stripped } else { stem.clone() };
            Toe { base, mtime: mtime(p), path: p.clone() }
        })
        .collect()
}

/// 老工程默认用文件夹名当工程名（去掉 2026-11-17_ 这类日期前缀）
fn folder_title(folder: &str) -> String {
    let b = folder.as_bytes();
    let dated = b.len() > 11
        && b[..10].iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
        && matches!(b[10], b'_' | b' ' | b'-');
    let t = if dated { folder[11..].trim() } else { folder.trim() };
    if t.is_empty() { folder.to_string() } else { t.to_string() }
}

/// 主 .toe：优先用 project.json 里记的 mainToe；否则在所有 .toe 里取最近修改的那份
fn main_toe(toes: &[Toe], prefer: Option<&str>) -> Option<PathBuf> {
    let pool: Vec<&Toe> = match prefer.filter(|b| toes.iter().any(|t| t.base == *b)) {
        Some(b) => toes.iter().filter(|t| t.base == b).collect(),
        None => toes.iter().collect(),
    };
    pool.into_iter().max_by_key(|t| t.mtime).map(|t| t.path.clone())
}

fn child_dirs_lower(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_lowercase()).collect())
        .unwrap_or_default()
}

/// 有 TD 原生工程结构或本模板结构
fn has_signature(subs: &[String]) -> bool {
    let td = ["backup", "audio", "chan", "geo", "image", "movie"];
    let tpl = ["assets", "tox", "scripts", "export"];
    let n = |set: &[&str]| subs.iter().filter(|s| set.contains(&s.as_str())).count();
    n(&td) >= 2 || n(&tpl) >= 2
}

/// 工程的 .toe 不在文件夹第一层时，最多往下找几层
const TOE_DEPTH: usize = 3;

#[tauri::command]
fn scan_projects(root: String, extra: Option<Vec<String>>) -> Result<Vec<Project>, String> {
    let root = PathBuf::from(&root);
    fs::read_dir(&root).map_err(|e| format!("{}（{}）", e, root.display()))?;
    let mut out = Vec::new();
    scan_root(&root, &mut out);
    // 导入的工程：不在根目录里，按登记的位置逐个读；找不到的（硬盘没插、被挪走）先跳过
    for e in extra.unwrap_or_default() {
        if let Some(p) = read_external(&root, Path::new(&e)) {
            out.push(p);
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(out)
}

/// p 在 root 里面（或就是 root）
fn inside(root: &Path, p: &Path) -> bool {
    match (root.canonicalize(), p.canonicalize()) {
        (Ok(r), Ok(d)) => d.starts_with(&r),
        _ => false,
    }
}

fn folder_name(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| dir.to_string_lossy().to_string())
}

/// 读一个登记在根目录以外的工程；已经在根目录里的不重复算
fn read_external(root: &Path, dir: &Path) -> Option<Project> {
    if !dir.is_dir() || inside(root, dir) {
        return None;
    }
    let mut p = read_project_ex(dir, &folder_name(dir), EXT_GROUP, true)?;
    p.external = true;
    Some(p)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportScan {
    /// 选的文件夹本来就在工程根目录里，不用导入
    inside_root: bool,
    projects: Vec<Project>,
}

/// 导入前先看一眼：选的文件夹本身是工程就返回它；不是就当上级文件夹，列出里面所有工程（批量导入）。
/// 只读，不写任何东西。拖进来的是 .toe 文件时，看它所在的文件夹。
#[tauri::command]
fn inspect_import(root: String, path: String) -> Result<ImportScan, String> {
    let mut dir = PathBuf::from(&path);
    if dir.is_file() {
        dir = dir.parent().map(Path::to_path_buf).ok_or("找不到这个文件所在的文件夹")?;
    }
    if !dir.is_dir() {
        return Err(format!("不是文件夹：{}", path));
    }
    let root = PathBuf::from(&root);
    if inside(&root, &dir) {
        return Ok(ImportScan { inside_root: true, projects: vec![] });
    }
    let mut out = Vec::new();
    // 分组：导入的工程按它原来所在的文件夹归类。
    // 选的文件夹本身是工程 → 用它上一级文件夹的名字；选的是分类文件夹 → 用这个文件夹的名字
    if let Some(mut p) = read_project(&dir, &folder_name(&dir), "") {
        p.group = dir.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| EXT_GROUP.to_string());
        out.push(p);
    } else {
        // 选的不是工程 → 当成一个分类文件夹：只认它直属的文件夹，每个算一个工程，都归到这个文件夹的名字下
        let here = folder_name(&dir);
        for (sub, folder) in child_dirs(&dir, false) {
            // 根目录本身、或者包着根目录的文件夹，不算导入
            if inside(&root, &sub) || inside(&sub, &root) {
                continue;
            }
            if let Some(p) = read_project_ex(&sub, &folder, &here, true) {
                out.push(p);
            }
        }
    }
    for p in &mut out {
        p.external = true;
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(ImportScan { inside_root: false, projects: out })
}

/// dir 直属的文件夹（跳过系统杂项、Backup、_ 和 . 开头的；top 表示 dir 是根目录，这时还跳过库文件夹）
fn child_dirs(dir: &Path, top: bool) -> Vec<(PathBuf, String)> {
    let Ok(rd) = fs::read_dir(dir) else { return vec![] };
    rd.flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| (e.path(), e.file_name().to_string_lossy().to_string()))
        .filter(|(_, n)| !skip_dir(n, top))
        .collect()
}

/// 只认两层：根目录直属的文件夹，要么是一个工程（里面直接有 .toe 或 project.json），要么是一个分类文件夹；
/// 分类文件夹直属的每个文件夹算一个工程，不再往下拆。工程里面更深的文件夹（组件、参考源文件、旧版本）都不单独算。
fn scan_root(root: &Path, out: &mut Vec<Project>) {
    for (sub, folder) in child_dirs(root, true) {
        // 里面直接有 .toe → 是工程
        if !toes_in(&sub).is_empty() {
            out.extend(read_project(&sub, &folder, ""));
            continue;
        }
        // 否则看它直属的文件夹：有工程 → 它是分类文件夹（哪怕它自己带着 project.json，比如被误导入过）
        let inner: Vec<Project> = child_dirs(&sub, false)
            .into_iter()
            .filter_map(|(proj, name)| read_project_ex(&proj, &name, &folder, true))
            .collect();
        if inner.is_empty() {
            out.extend(read_project(&sub, &folder, "")); // 只有 project.json 的空工程
        } else {
            out.extend(inner);
        }
    }
}

/// 文件夹第一层没有 .toe 时，往下找：取"里面有 .toe 的文件夹"中最近保存过的那一个，返回它里面的 .toe
fn nested_toes(dir: &Path) -> Vec<Toe> {
    let mut budget = 400usize;
    let mut best: Option<(u64, PathBuf)> = None;
    let mut queue = std::collections::VecDeque::from([(dir.to_path_buf(), 0usize)]);
    while let Some((d, depth)) = queue.pop_front() {
        if depth > 0 {
            if let Some(latest) = toes_in(&d).iter().map(|t| t.mtime).max() {
                if best.as_ref().map(|(m, _)| latest > *m).unwrap_or(true) {
                    best = Some((latest, d.clone()));
                }
            }
        }
        if depth >= TOE_DEPTH {
            continue;
        }
        for (sub, _) in child_dirs(&d, false) {
            if budget == 0 {
                break;
            }
            budget -= 1;
            queue.push_back((sub, depth + 1));
        }
    }
    best.map(|(_, d)| toes_in(&d)).unwrap_or_default()
}

/// 这个文件夹的 .toe：先看第一层，没有再往下找
fn project_toes(dir: &Path) -> Vec<Toe> {
    let t = toes_in(dir);
    if t.is_empty() {
        nested_toes(dir)
    } else {
        t
    }
}

/// 根目录第一层用：里面直接有 .toe 或 project.json 才算工程（否则当分类文件夹）
fn read_project(dir: &Path, folder: &str, group: &str) -> Option<Project> {
    read_project_ex(dir, folder, group, false)
}

/// deep：.toe 不在第一层也算工程（分类文件夹里的、导入的都这样）
fn read_project_ex(dir: &Path, folder: &str, group: &str, deep: bool) -> Option<Project> {
    let has_meta = dir.join("project.json").is_file();
    let mut toes = toes_in(dir);
    if toes.is_empty() && (deep || has_meta) {
        toes = nested_toes(dir);
    }
    if toes.is_empty() && !has_meta {
        return None; // 不是工程文件夹（只剩 Backup 的也不算）
    }
    let meta = read_meta(dir);
    let prefer = meta.as_ref().and_then(|m| m.get("mainToe")).and_then(|v| v.as_str()).map(String::from);
    let toe = main_toe(&toes, prefer.as_deref());
    let mut bases: Vec<String> = toes.iter().map(|t| t.base.clone()).collect();
    bases.sort();
    bases.dedup();

    let subs = child_dirs_lower(dir);
    let confidence = if has_meta {
        "meta"
    } else if has_signature(&subs) {
        "likely"
    } else {
        "maybe"
    };

    // 最早的 .toe：工程文件夹和 Backup 里一起看
    let mut earliest = toes.iter().map(|t| t.mtime).filter(|t| *t > 0).min().unwrap_or(0);
    if let Some(bk) = fs::read_dir(dir)
        .ok()
        .and_then(|rd| rd.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case("backup")))
    {
        for t in toes_in(&bk.path()) {
            if t.mtime > 0 && (earliest == 0 || t.mtime < earliest) {
                earliest = t.mtime;
            }
        }
    }

    let thumb = dir.join("thumb.jpg");
    let has_thumb = thumb.is_file();
    let pref = meta.as_ref().and_then(|m| m.get("thumbFile")).and_then(|v| v.as_str()).map(String::from);
    let (shown, thumb_auto) = resolve_thumb(dir, pref.as_deref());
    let name = meta
        .as_ref()
        .and_then(|m| m.get("name"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .unwrap_or_else(|| folder_title(folder));
    let modified = [
        toe.as_ref().map(|t| mtime(t)).unwrap_or(0),
        mtime(&dir.join("project.json")),
        if has_thumb { mtime(&thumb) } else { 0 },
        mtime(dir),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    Some(Project {
        path: dir.to_string_lossy().to_string(),
        folder: folder.to_string(),
        group: group.to_string(),
        name,
        toe: toe.map(|t| t.to_string_lossy().to_string()),
        toe_bases: bases,
        confidence,
        earliest,
        thumb_mtime: shown.as_ref().map(|p| mtime(p)).unwrap_or(0),
        thumb: shown.map(|p| p.to_string_lossy().to_string()),
        thumb_auto,
        modified,
        has_meta,
        meta: meta.unwrap_or(Value::Null),
        external: false,
        folder_icon: has_folder_icon(dir),
    })
}

/* ---------------- 预览图 ---------------- */

const PREVIEW_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif"];

fn is_preview_image(name: &str) -> bool {
    let l = name.to_lowercase();
    !l.starts_with("._") && l != ICON_FILE && l.rsplit_once('.').map(|(_, e)| PREVIEW_EXTS.contains(&e)).unwrap_or(false)
}

/// 用哪张图当预览，以及它是不是自动找的。
/// 顺序：用户在 project.json 里指定的（thumbFile）→ thumb.jpg（TD 保存时存的）→ 在文件夹里自动找一张现成的图。
/// thumbFile 是 "-" 表示用户说过"不用预览图"，这时不自动找。
fn resolve_thumb(dir: &Path, pref: Option<&str>) -> (Option<PathBuf>, bool) {
    if let Some(f) = pref.filter(|f| !f.is_empty() && *f != "-") {
        // 只认工程文件夹里面的相对路径
        let rel = Path::new(f);
        if rel.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
            let p = dir.join(rel);
            if p.is_file() && is_preview_image(&folder_name(&p)) {
                return (Some(p), false);
            }
        }
    }
    let t = dir.join("thumb.jpg");
    if t.is_file() {
        return (Some(t), false);
    }
    if pref == Some("-") {
        return (None, false);
    }
    let found = find_preview(dir);
    let auto = found.is_some();
    (found, auto)
}

/// 文件夹名加分 / 减分：导出、截图一类的更可能是成品画面，素材、贴图一类的多半不是
fn preview_dir_bonus(lower: &str) -> i32 {
    const GOOD: &[&str] = &["export", "exports", "output", "outputs", "render", "renders", "screenshots", "screenshot", "录制", "导出", "输出", "截图", "成品"];
    const BAD: &[&str] = &["assets", "asset", "tex", "texture", "textures", "maps", "hdri", "images", "image", "素材", "贴图", "tdimportcache"];
    if GOOD.contains(&lower) {
        30
    } else if BAD.contains(&lower) {
        -30
    } else {
        0
    }
}

/// 旧工程没有 thumb.jpg 时，在工程文件夹里找一张现成的图当预览：最多往下两层、最多看 600 个条目，不会拖慢扫描。
/// 名字像截图 / 预览 / TD 导出的、放在导出文件夹里的优先，同分取最近修改的。只读，不往文件夹里写东西。
fn find_preview(dir: &Path) -> Option<PathBuf> {
    const KEYS: &[&str] = &["thumb", "preview", "cover", "screenshot", "截图", "预览", "封面", "效果"];
    let mut budget = 600usize;
    let mut best: Option<(i32, u64, PathBuf)> = None;
    let mut queue = std::collections::VecDeque::from([(dir.to_path_buf(), 0usize, 0i32)]);
    while let Some((d, depth, bonus)) = queue.pop_front() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            if budget == 0 {
                return best.map(|b| b.2);
            }
            budget -= 1;
            let name = e.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if depth < 2 && !skip_dir(&name, false) {
                    queue.push_back((e.path(), depth + 1, bonus + preview_dir_bonus(&lower)));
                }
                continue;
            }
            if !ft.is_file() || !is_preview_image(&name) {
                continue;
            }
            // 太小的多半是图标，太大的加载慢
            let len = e.metadata().map(|m| m.len()).unwrap_or(0);
            if !(8 * 1024..=40 * 1024 * 1024).contains(&len) {
                continue;
            }
            let mut score = bonus + if depth == 0 { 20 } else { 0 };
            if KEYS.iter().any(|k| lower.contains(k)) {
                score += 50;
            }
            if lower.starts_with("tdmovieout") {
                score += 40;
            }
            let p = e.path();
            let mt = mtime(&p);
            if best.as_ref().map(|(s, m, _)| (score, mt) > (*s, *m)).unwrap_or(true) {
                best = Some((score, mt, p));
            }
        }
    }
    best.map(|b| b.2)
}

/// 用户自己定预览图。file：None = 不用预览图；"" = 恢复自动；其它 = 一张图片的位置。
/// 图片在工程文件夹里就只记相对位置；在外面就复制进来（thumb-custom.扩展名）。记在 project.json 的 thumbFile 里
fn set_thumb_at(dir: &Path, file: Option<&str>) -> Result<(), String> {
    let val = match file {
        None => "-".to_string(),
        Some("") => String::new(),
        Some(f) => {
            let src = Path::new(f);
            if !src.is_file() || !is_preview_image(&folder_name(src)) {
                return Err("要选一张图片（JPG、PNG、WebP、BMP、GIF）".into());
            }
            let (s, d) = (src.canonicalize().map_err(|e| e.to_string())?, dir.canonicalize().map_err(|e| e.to_string())?);
            match s.strip_prefix(&d) {
                Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
                Err(_) => {
                    let ext = folder_name(src).rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
                    let name = format!("thumb-custom.{}", ext);
                    fs::copy(src, dir.join(&name)).map_err(|e| e.to_string())?;
                    name
                }
            }
        }
    };
    save_meta(dir.to_string_lossy().to_string(), json!({ "thumbFile": val }))
}

#[tauri::command]
fn set_thumb(path: String, file: Option<String>) -> Result<(), String> {
    let dir = PathBuf::from(plain(&check_project(Path::new(&path))?));
    set_thumb_at(&dir, file.as_deref())
}

/// 模板 .toe：开发时在 src-tauri/resources/template/，打包后在安装目录的资源里
fn template_toe(app: &AppHandle) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(r) = app.path().resource_dir() {
        candidates.push(r.join("resources").join("template").join("template.toe"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join("template").join("template.toe"));
    candidates.into_iter().find(|p| p.is_file())
}

/* ---------------- 自己做的工程模板 ---------------- */

/// 自己做的模板放在软件的数据目录里：templates/模板名/ 就是一个真实的文件夹，新建工程时整个复制过去
fn templates_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("templates"))
}

const TEMPLATES_README: &str = "这里的每个文件夹就是一个工程模板，文件夹名就是模板名。\r\n\r\n新建工程时选它，软件会把整个文件夹（里面的子文件夹和文件）复制成新工程：\r\n- 文件夹里的 .toe 会改名成「工程名.toe」；有好几个 .toe 时改最近保存的那个。\r\n- 没有 .toe 的话，用软件自带的那份（里面有保存时自动存缩略图的脚本）。\r\n- project.json、thumb.jpg、Backup 文件夹不会复制，新工程会有自己的。\r\n\r\n做模板的两种办法：\r\n1. 在启动器里打开一个工程的详情，点「存为模板」：会存下它的文件夹结构和最新的 .toe。\r\n2. 直接在这里新建一个文件夹，把想要的子文件夹和文件放进去。\r\n\r\n改名、删除模板：直接在这里改文件夹名、删文件夹。\r\n";

/// 模板名 / 模板里的文件夹名：不能有系统不允许的字符，也不能是 . 或 ..
fn safe_name(name: &str) -> Result<String, String> {
    let n = name.trim().trim_end_matches(['.', ' ']).to_string();
    if n.is_empty() || n == "." || n == ".." || n.contains(BAD_CHARS) || n.starts_with('_') || n.starts_with('.') {
        return Err("名字不能为空，不能以 _ 或 . 开头，也不能有 : ? * / \\ 这些字符".into());
    }
    Ok(n)
}

/// 复制模板 / 存模板时都不带走的东西
fn template_skip(name: &str, is_dir: bool) -> bool {
    let l = name.to_lowercase();
    system_junk(name).is_some()
        || l == "desktop.ini"
        || l == ICON_FILE
        || if is_dir { l == "backup" || l == "node_modules" } else { l == "project.json" || l == "thumb.jpg" || l.starts_with("crashautosave") }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Template {
    name: String,
    has_toe: bool,
    /// 第一层的内容，文件夹以 / 结尾，给"将会创建"的预览用
    entries: Vec<String>,
}

fn read_templates(dir: &Path) -> Vec<Template> {
    let Ok(rd) = fs::read_dir(dir) else { return vec![] };
    let mut out: Vec<Template> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if safe_name(&name).ok()? != name {
                return None;
            }
            let p = e.path();
            let mut entries: Vec<String> = fs::read_dir(&p)
                .map(|rd| {
                    rd.flatten()
                        .filter_map(|c| {
                            let n = c.file_name().to_string_lossy().to_string();
                            let d = c.path().is_dir();
                            (!template_skip(&n, d) && !n.to_lowercase().ends_with(".toe")).then(|| if d { format!("{}/", n) } else { n })
                        })
                        .collect()
                })
                .unwrap_or_default();
            // 文件夹排前面，各自按名字
            entries.sort_by(|a, b| b.ends_with('/').cmp(&a.ends_with('/')).then(a.to_lowercase().cmp(&b.to_lowercase())));
            Some(Template { has_toe: !toes_in(&p).is_empty(), name, entries })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

#[tauri::command]
fn list_templates(app: AppHandle) -> Vec<Template> {
    templates_dir(&app).map(|d| read_templates(&d)).unwrap_or_default()
}

fn prepare_templates_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("_说明.txt"), TEMPLATES_README).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_templates_dir(app: AppHandle) -> Result<String, String> {
    let dir = templates_dir(&app).ok_or("找不到软件的数据目录")?;
    prepare_templates_dir(&dir)?;
    let s = dir.to_string_lossy().to_string();
    app.opener().open_path(s.clone(), None::<&str>).map_err(|e| e.to_string())?;
    Ok(s)
}

/// 一次最多复制这么多个文件，防止不小心把一个巨大的文件夹当成模板
const TEMPLATE_MAX_FILES: usize = 5000;

fn copy_tree(src: &Path, dst: &Path, depth: usize, count: &mut usize) -> Result<(), String> {
    let rd = fs::read_dir(src).map_err(|e| e.to_string())?;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let ft = e.file_type().map_err(|e| e.to_string())?;
        if ft.is_symlink() || template_skip(&name, ft.is_dir()) {
            continue;
        }
        let to = dst.join(&name);
        if ft.is_dir() {
            if depth >= 12 {
                continue;
            }
            fs::create_dir_all(&to).map_err(|e| e.to_string())?;
            copy_tree(&e.path(), &to, depth + 1, count)?;
        } else {
            // 第一层的 .toe 由调用方处理（要改名）
            if depth == 0 && name.to_lowercase().ends_with(".toe") {
                continue;
            }
            *count += 1;
            if *count > TEMPLATE_MAX_FILES {
                return Err(format!("这个模板里的文件太多了（超过 {} 个）", TEMPLATE_MAX_FILES));
            }
            fs::copy(e.path(), &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 把模板文件夹复制成新工程；模板里有 .toe 就把最近保存的那个改名成「工程名.toe」，返回它的位置
fn copy_template(tpl: &Path, dir: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let mut count = 0;
    copy_tree(tpl, dir, 0, &mut count)?;
    let toes = toes_in(tpl);
    let Some(main) = main_toe(&toes, None) else { return Ok(None) };
    let main_base = toes.iter().find(|t| t.path == main).map(|t| t.base.clone()).unwrap_or_default();
    // 别的名字的 .toe 原样带上（各取最新的一份），主文件改名
    let mut bases: Vec<&str> = toes.iter().map(|t| t.base.as_str()).filter(|b| *b != main_base).collect();
    bases.sort();
    bases.dedup();
    for b in bases {
        if let Some(p) = main_toe(&toes, Some(b)) {
            fs::copy(&p, dir.join(format!("{}.toe", b))).map_err(|e| e.to_string())?;
        }
    }
    let dst = dir.join(format!("{}.toe", name));
    fs::copy(&main, &dst).map_err(|e| e.to_string())?;
    Ok(Some(dst))
}

/// 把一个工程存成模板：只存文件夹结构（空文件夹）和最新的主 .toe，素材和导出的文件不带
fn save_template_at(templates: &Path, project: &Path, name: &str) -> Result<(), String> {
    let name = safe_name(name)?;
    let dst = templates.join(&name);
    if dst.exists() {
        return Err(format!("已经有叫「{}」的模板了", name));
    }
    fs::create_dir_all(&dst).map_err(|e| e.to_string())?;
    fn dirs_only(src: &Path, dst: &Path, depth: usize) {
        if depth >= 6 {
            return;
        }
        let Ok(rd) = fs::read_dir(src) else { return };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if !is_dir || skip_dir(&n, false) || template_skip(&n, true) {
                continue;
            }
            let to = dst.join(&n);
            if fs::create_dir_all(&to).is_ok() {
                dirs_only(&e.path(), &to, depth + 1);
            }
        }
    }
    dirs_only(project, &dst, 0);
    let prefer = read_meta(project).and_then(|m| m.get("mainToe").and_then(|v| v.as_str()).map(String::from));
    if let Some(toe) = main_toe(&toes_in(project), prefer.as_deref()) {
        fs::copy(&toe, dst.join("template.toe")).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn save_as_template(app: AppHandle, path: String, name: String) -> Result<(), String> {
    let project = check_project(Path::new(&path))?;
    let dir = templates_dir(&app).ok_or("找不到软件的数据目录")?;
    prepare_templates_dir(&dir)?;
    save_template_at(&dir, &project, &name)
}

/// 新建时勾选的一个组件：src 是 .tox 文件或一个文件夹；load 是文件夹里要自动载入的 .tox（相对路径）
#[derive(Deserialize)]
struct CompReq {
    name: String,
    src: String,
    #[serde(default)]
    load: Vec<String>,
}

/// 软件自带的组件（现在只有缩略图截取）
fn builtin_component(app: &AppHandle, id: &str) -> Option<PathBuf> {
    let file = match id {
        "thumbnail" => "TDL_Thumbnail.tox",
        _ => return None,
    };
    let mut candidates = Vec::new();
    if let Ok(r) = app.path().resource_dir() {
        candidates.push(r.join("resources").join("components").join(file));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join("components").join(file));
    candidates.into_iter().find(|p| p.is_file())
}

#[tauri::command(async)]
fn create_project(app: AppHandle, root: String, folder: String, name: String, created: String, template: Option<String>, components: Option<Vec<CompReq>>, case: Option<bool>, order: Option<bool>) -> Result<Created, String> {
    let mut comps = Vec::new();
    for c in components.unwrap_or_default() {
        let src = match c.src.strip_prefix("builtin:") {
            // 自带组件还没随软件打包时直接跳过，不拦着新建
            Some(id) => match builtin_component(&app, id) {
                Some(p) => p,
                None => continue,
            },
            None => PathBuf::from(&c.src),
        };
        comps.push((c.name, src, c.load));
    }
    // 选了自己做的模板：必须是模板文件夹里的一个文件夹
    let tpl_dir = match template.filter(|t| !t.is_empty()) {
        Some(t) => {
            let d = templates_dir(&app).ok_or("找不到软件的数据目录")?.join(safe_name(&t)?);
            if !d.is_dir() {
                return Err(format!("找不到模板「{}」", t));
            }
            Some(d)
        }
        None => None,
    };
    let (n, c) = (name.clone(), created.clone());
    let made = create_project_at(root, folder, name, created, template_toe(&app), tpl_dir, comps, case.unwrap_or(false))?;
    if order.unwrap_or(false) {
        // 定制工程：多 docs（需求单、报价、验收记录）和 deliver（发给客户的压缩包），加一份交付说明
        let _ = order_extras(Path::new(&made.path), &n, &c);
    }
    Ok(made)
}

const ORDER_README: &str = include_str!("../../docs/order-readme.md");

fn order_extras(dir: &Path, name: &str, created: &str) -> Result<(), String> {
    fs::create_dir_all(dir.join("docs")).map_err(|e| e.to_string())?;
    fs::create_dir_all(dir.join("deliver")).map_err(|e| e.to_string())?;
    if !dir.join("README.md").exists() {
        fs::write(dir.join("README.md"), ORDER_README.replace("{{name}}", name).replace("{{date}}", created)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 把 src（文件或文件夹）整个复制到 dst；跳过系统杂项和启动器自己的标记文件
fn copy_all(src: &Path, dst: &Path) -> Result<(), String> {
    if src.is_dir() {
        fs::create_dir_all(dst).map_err(|e| e.to_string())?;
        for e in fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let l = name.to_lowercase();
            if system_junk(&name).is_some() || l == "desktop.ini" || l == ICON_FILE || l == "project.json" || l == "backup" {
                continue;
            }
            copy_all(&e.path(), &dst.join(&name))?;
        }
        Ok(())
    } else {
        fs::copy(src, dst).map(|_| ()).map_err(|e| format!("复制 {} 失败：{}", src.display(), e))
    }
}

/// 把勾选的组件复制进工程的 tox 文件夹，返回要记进 project.json 的清单。
/// 工程里的载入器第一次打开时按这份清单把 files 里的 .tox 载入，然后把 loaded 改成 true。
fn install_components(dir: &Path, comps: &[(String, PathBuf, Vec<String>)]) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    if comps.is_empty() {
        return Ok(out);
    }
    let tox = dir.join("tox");
    fs::create_dir_all(&tox).map_err(|e| e.to_string())?;
    for (name, src, load) in comps {
        if !src.exists() {
            return Err(format!("组件「{}」的文件找不到了：{}", name, src.display()));
        }
        let fname = src.file_name().map(|n| n.to_string_lossy().to_string()).ok_or("组件路径不对")?;
        let dst = tox.join(&fname);
        copy_all(src, &dst)?;
        let files: Vec<String> = if src.is_file() {
            vec![format!("tox/{}", fname)]
        } else {
            load.iter()
                .map(|l| l.replace('\\', "/"))
                .filter(|l| !l.contains("..") && dst.join(l).is_file())
                .map(|l| format!("tox/{}/{}", fname, l))
                .collect()
        };
        out.push(json!({ "name": name, "files": files, "loaded": false }));
    }
    Ok(out)
}

const CASE_README: &str = include_str!("../../docs/case-readme.md");

fn create_project_at(root: String, folder: String, name: String, created: String, tpl: Option<PathBuf>, tpl_dir: Option<PathBuf>, comps: Vec<(String, PathBuf, Vec<String>)>, case: bool) -> Result<Created, String> {
    let folder = folder.trim().trim_end_matches(['.', ' ']).to_string();
    let name = name.trim().to_string();
    if folder.is_empty() || name.is_empty() {
        return Err("工程名称不能为空".into());
    }
    if folder.contains(BAD_CHARS) || name.contains(BAD_CHARS) {
        return Err("名称里有系统不允许的字符".into());
    }
    let root = PathBuf::from(root);
    if !root.is_dir() {
        return Err("工程根目录不存在".into());
    }
    let dir = root.join(&folder);
    if dir.exists() {
        return Err(format!("已经有同名文件夹：{}", folder));
    }
    let mut toe = None;
    if let Some(td) = &tpl_dir {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // 复制到一半出错就把半成品删掉，不留一个残缺的工程
        match copy_template(td, &dir, &name) {
            Ok(t) => toe = t.map(|p| p.to_string_lossy().to_string()),
            Err(e) => {
                let _ = fs::remove_dir_all(&dir);
                return Err(e);
            }
        }
    } else {
        for sub in SUBDIRS {
            fs::create_dir_all(dir.join(sub)).map_err(|e| e.to_string())?;
        }
    }
    // 组件复制到一半出错，同样不留半成品
    let components = match install_components(&dir, &comps) {
        Ok(c) => c,
        Err(e) => {
            let _ = fs::remove_dir_all(&dir);
            return Err(e);
        }
    };
    if case {
        // 案例工程：多一个放演示视频的 demo 文件夹，和一份给买家看的说明
        fs::create_dir_all(dir.join("demo")).map_err(|e| e.to_string())?;
        if !dir.join("README.md").exists() {
            fs::write(dir.join("README.md"), CASE_README.replace("{{name}}", &name).replace("{{date}}", &created)).map_err(|e| e.to_string())?;
        }
    }
    let mut meta = json!({
        "name": name,
        "tags": [],
        "status": "进行中",
        "client": "",
        "note": "",
        "created": created,
    });
    if !components.is_empty() {
        meta["components"] = Value::Array(components);
    }
    fs::write(dir.join("project.json"), serde_json::to_string_pretty(&meta).unwrap()).map_err(|e| e.to_string())?;
    // 模板里没有 .toe（或用的是默认模板）：用软件自带的那份
    if let (None, Some(tpl)) = (&toe, tpl) {
        let dst = dir.join(format!("{}.toe", name));
        fs::copy(&tpl, &dst).map_err(|e| e.to_string())?;
        toe = Some(dst.to_string_lossy().to_string());
    }
    Ok(Created { path: dir.to_string_lossy().to_string(), toe })
}

/// 把界面上改的字段合并进 project.json；文件里其它字段原样保留
#[tauri::command]
fn save_meta(path: String, meta: Value) -> Result<(), String> {
    let dir = PathBuf::from(path);
    if !dir.is_dir() {
        return Err("工程文件夹不存在".into());
    }
    let mut obj: Map<String, Value> = match read_meta(&dir) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    if let Value::Object(new) = meta {
        for (k, v) in new {
            obj.insert(k, v);
        }
    }
    let text = serde_json::to_string_pretty(&Value::Object(obj)).unwrap();
    let tmp = dir.join("project.json.tmp");
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, dir.join("project.json")).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_project(app: AppHandle, path: String) -> Result<(), String> {
    let dir = Path::new(&path);
    let prefer = read_meta(dir).and_then(|m| m.get("mainToe").and_then(|v| v.as_str()).map(String::from));
    let toe = main_toe(&project_toes(dir), prefer.as_deref()).ok_or("这个工程里没有 .toe 文件")?;
    app.opener()
        .open_path(toe.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string())
}

/// 检查 path 是根目录里面的一个工程文件夹（有 .toe 或 project.json），防止误删别的东西
fn check_deletable(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|_| "工程根目录不存在".to_string())?;
    let dir = path.canonicalize().map_err(|_| "工程文件夹不存在".to_string())?;
    if !dir.is_dir() || dir == root || !dir.starts_with(&root) {
        return Err("只能操作工程根目录里的工程文件夹".into());
    }
    if project_toes(&dir).is_empty() && !dir.join("project.json").is_file() {
        return Err("这个文件夹不像工程（没有 .toe 也没有 project.json），没有动它".into());
    }
    Ok(dir)
}

/// 检查 path 是一个工程文件夹（不要求在根目录里），给导入的工程用
fn check_project(path: &Path) -> Result<PathBuf, String> {
    let dir = path.canonicalize().map_err(|_| "工程文件夹不存在".to_string())?;
    if !dir.is_dir() || (project_toes(&dir).is_empty() && !dir.join("project.json").is_file()) {
        return Err("这个文件夹不像工程（没有 .toe 也没有 project.json），没有动它".into());
    }
    Ok(dir)
}

/// 删除工程：整个文件夹移到系统回收站，可以恢复
#[tauri::command]
fn delete_project(root: String, path: String) -> Result<(), String> {
    let dir = check_deletable(Path::new(&root), Path::new(&path))?;
    trash::delete(&dir).map_err(|e| format!("移到回收站失败：{e}"))
}

/// 去掉 Windows canonicalize 加的 \\?\ 前缀
fn plain(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    s.strip_prefix(r"\\?\").map(String::from).unwrap_or(s)
}

/// 把工程文件夹整个移到根目录下的另一个分类文件夹（dest 是相对根目录的路径，空 = 根目录本身）
#[tauri::command]
fn move_project(root: String, path: String, dest: String, external: Option<bool>) -> Result<String, String> {
    // 导入的工程在根目录外面：移进分类文件夹就等于把它收进根目录
    let dir = if external.unwrap_or(false) { check_project(Path::new(&path))? } else { check_deletable(Path::new(&root), Path::new(&path))? };
    let root_c = Path::new(&root).canonicalize().map_err(|_| "工程根目录不存在".to_string())?;
    let rel = dest.trim().trim_matches(['/', '\\']);
    let mut target_dir = root_c.clone();
    if !rel.is_empty() {
        for seg in rel.split(['/', '\\']) {
            let seg = seg.trim();
            if seg.is_empty() || seg == "." || seg == ".." || seg.contains(BAD_CHARS) {
                return Err(format!("分类文件夹名字不合法：{}", rel));
            }
            target_dir.push(seg);
        }
    }
    if target_dir.starts_with(&dir) {
        return Err("不能把工程移到它自己里面".into());
    }
    let fname = dir.file_name().ok_or("工程文件夹名字不对")?.to_os_string();
    let target = target_dir.join(&fname);
    if target == dir {
        return Err("已经在这个分类里了".into());
    }
    if target.exists() {
        return Err(format!("目标里已经有同名文件夹「{}」", fname.to_string_lossy()));
    }
    fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
    fs::rename(&dir, &target).map_err(|e| match e.raw_os_error() {
        Some(5) | Some(32) => "文件被占用（可能正在 TD 里打开），没有移动".to_string(),
        Some(17) | Some(18) => "不能跨硬盘移动，请在系统里手动移动".to_string(),
        _ => e.to_string(),
    })?;
    Ok(plain(&target))
}

/* ---------------- 文件夹图标 ---------------- */

/// 这个文件夹的图标是启动器换上的（图标文件在，desktop.ini 也指向它）
fn has_folder_icon(dir: &Path) -> bool {
    dir.join(ICON_FILE).is_file() && ini_is_ours(dir)
}

fn ini_is_ours(dir: &Path) -> bool {
    fs::read(dir.join("desktop.ini")).map(|b| String::from_utf8_lossy(&b).contains(ICON_FILE)).unwrap_or(false)
}

/* ---------------- 主题 ---------------- */

/// 主题制作规范和模板：编译时带进软件里，第一次打开主题文件夹时放进去
const THEME_SPEC: &str = include_str!("../../docs/theme-spec.md");
const THEME_TEMPLATE: &str = include_str!("../../docs/theme-template.json");
const THEME_MAX: u64 = 64 * 1024;

/// 自己做的主题放在软件的数据目录里（不在安装目录，重装、升级都不会丢）
fn themes_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("themes"))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UserTheme {
    file: String,
    /// 主题文件所在的文件夹，界面用它找主题自带的标志图片
    dir: String,
    data: Value,
    /// 读不出来的原因；内容对不对由界面检查
    error: Option<String>,
}

/// 读主题文件夹里的 .json（`_` 开头的是模板，跳过）。只负责读和解析，颜色对不对由界面检查
fn read_user_themes(dir: &Path) -> Vec<UserTheme> {
    let Ok(rd) = fs::read_dir(dir) else { return vec![] };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            p.is_file() && n.ends_with(".json") && !n.starts_with('_') && !n.starts_with('.')
        })
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let file = folder_name(&p);
            let parsed: Result<Value, String> = if fs::metadata(&p).map(|m| m.len()).unwrap_or(0) > THEME_MAX {
                Err("文件太大（超过 64 KB）".into())
            } else {
                fs::read_to_string(&p)
                    .map_err(|_| "读不出来，要用 UTF-8 保存".to_string())
                    .and_then(|s| serde_json::from_str::<Value>(s.trim_start_matches('\u{feff}')).map_err(|e| format!("不是合法的 JSON（第 {} 行）", e.line())))
            };
            let (data, error) = match parsed {
                Ok(v) => (v, None),
                Err(e) => (Value::Null, Some(e)),
            };
            UserTheme { file, dir: dir.to_string_lossy().to_string(), data, error }
        })
        .collect()
}

#[tauri::command]
fn list_user_themes(app: AppHandle) -> Vec<UserTheme> {
    themes_dir(&app).map(|d| read_user_themes(&d)).unwrap_or_default()
}

/// 建好主题文件夹：规范每次都换成最新的，模板只在没有时才放（不覆盖用户改过的）
fn prepare_themes_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("主题制作规范.md"), THEME_SPEC).map_err(|e| e.to_string())?;
    let tpl = dir.join("_template.json");
    if !tpl.exists() {
        fs::write(&tpl, THEME_TEMPLATE).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn open_themes_dir(app: AppHandle) -> Result<String, String> {
    let dir = themes_dir(&app).ok_or("找不到软件的数据目录")?;
    prepare_themes_dir(&dir)?;
    let s = dir.to_string_lossy().to_string();
    app.opener().open_path(s.clone(), None::<&str>).map_err(|e| e.to_string())?;
    Ok(s)
}

/* ---------------- 自定义标志 ---------------- */

/// 自定义标志存在软件的数据目录里（不在安装目录，重装、升级都不会丢）
fn custom_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("custom"))
}

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "svg", "gif", "bmp", "ico"];
const IMAGE_MAX: u64 = 8 * 1024 * 1024;

/// 读一张图片的原始内容给界面用（界面把它画到文件夹图标上）。只读图片格式，最大 8 MB
#[tauri::command]
fn read_image(path: String) -> Result<Vec<u8>, String> {
    let p = Path::new(&path);
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        return Err("只支持 PNG、JPG、WebP、SVG 这几种图片".into());
    }
    let len = fs::metadata(p).map_err(|e| e.to_string())?.len();
    if len > IMAGE_MAX {
        return Err("图片太大了（超过 8 MB），换一张小一点的".into());
    }
    fs::read(p).map_err(|e| e.to_string())
}

/// 把界面合成好的标志（logo.png）和文件夹图标（folder.ico）存进 dir；内容不对就不存
fn write_custom_logo(dir: &Path, logo: &[u8], ico: &[u8]) -> Result<PathBuf, String> {
    if !logo.starts_with(&[0x89, b'P', b'N', b'G']) || !ico.starts_with(&[0, 0, 1, 0]) || ico.len() < 22 {
        return Err("标志文件格式不对，没有保存".into());
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("folder.ico"), ico).map_err(|e| e.to_string())?;
    let png = dir.join("logo.png");
    fs::write(&png, logo).map_err(|e| e.to_string())?;
    Ok(png)
}

#[tauri::command]
fn save_custom_logo(app: AppHandle, logo: Vec<u8>, ico: Vec<u8>) -> Result<String, String> {
    let dir = custom_dir(&app).ok_or("找不到软件的数据目录")?;
    write_custom_logo(&dir, &logo, &ico).map(|p| p.to_string_lossy().to_string())
}

/// 恢复默认标志
#[tauri::command]
fn clear_custom_logo(app: AppHandle) -> Result<(), String> {
    let dir = custom_dir(&app).ok_or("找不到软件的数据目录")?;
    for f in ["logo.png", "folder.ico"] {
        let p = dir.join(f);
        if p.is_file() {
            fs::remove_file(&p).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 当前的自定义标志（logo.png 的位置和修改时间）；没设过就是 None
#[tauri::command]
fn custom_logo(app: AppHandle) -> Option<(String, u64)> {
    let p = custom_dir(&app)?.join("logo.png");
    p.is_file().then(|| (p.to_string_lossy().to_string(), mtime(&p)))
}

/// 图标文件：先用自定义的；没有就用自带的（开发时在 src-tauri/resources/template/，打包后在安装目录的资源里）
fn folder_ico(app: &AppHandle) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(d) = custom_dir(app) {
        candidates.push(d.join("folder.ico"));
    }
    if let Ok(r) = app.path().resource_dir() {
        candidates.push(r.join("resources").join("template").join("folder.ico"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join("template").join("folder.ico"));
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(windows)]
mod winattr {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    pub const READONLY: u32 = 0x1;
    pub const HIDDEN: u32 = 0x2;
    pub const SYSTEM: u32 = 0x4;
    const INVALID: u32 = 0xFFFF_FFFF;
    const SHCNE_UPDATEITEM: i32 = 0x0000_2000;
    const SHCNF_PATHW: u32 = 0x0005;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileAttributesW(name: *const u16) -> u32;
        fn SetFileAttributesW(name: *const u16, attrs: u32) -> i32;
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHChangeNotify(event: i32, flags: u32, item1: *const core::ffi::c_void, item2: *const core::ffi::c_void);
    }

    fn wide(p: &Path) -> Vec<u16> {
        p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    /// 加上 / 去掉几个属性位，其它属性不动
    pub fn change(p: &Path, add: u32, remove: u32) {
        let w = wide(p);
        unsafe {
            let cur = GetFileAttributesW(w.as_ptr());
            if cur != INVALID {
                SetFileAttributesW(w.as_ptr(), (cur | add) & !remove);
            }
        }
    }

    /// 告诉资源管理器这个文件夹变了，图标马上刷新
    pub fn refresh(p: &Path) {
        let w = wide(p);
        unsafe { SHChangeNotify(SHCNE_UPDATEITEM, SHCNF_PATHW, w.as_ptr() as *const core::ffi::c_void, core::ptr::null()) }
    }
}
#[cfg(not(windows))]
mod winattr {
    use std::path::Path;
    pub const READONLY: u32 = 0x1;
    pub const HIDDEN: u32 = 0x2;
    pub const SYSTEM: u32 = 0x4;
    pub fn change(_: &Path, _: u32, _: u32) {}
    pub fn refresh(_: &Path) {}
}

/// 给工程文件夹换上 / 换回图标。只动两个隐藏文件（desktop.ini 和 .td-folder.ico），工程本身的文件不碰。
/// 文件夹原来就有别人写的 desktop.ini 时不改它。
fn apply_folder_icon(dir: &Path, ico: Option<&Path>, on: bool) -> Result<(), String> {
    let ini = dir.join("desktop.ini");
    let icon = dir.join(ICON_FILE);
    if ini.is_file() && !ini_is_ours(dir) {
        return Err("这个文件夹已经有自己的图标设置（desktop.ini），没有改动".into());
    }
    // 隐藏 + 系统属性的文件不能直接覆盖或删除，先去掉属性
    for f in [&ini, &icon] {
        if f.is_file() {
            winattr::change(f, 0, winattr::HIDDEN | winattr::SYSTEM | winattr::READONLY);
        }
    }
    if on {
        let ico = ico.ok_or("安装目录里找不到图标文件 folder.ico")?;
        fs::copy(ico, &icon).map_err(|e| e.to_string())?;
        fs::write(&ini, ICON_INI).map_err(|e| e.to_string())?;
        winattr::change(&icon, winattr::HIDDEN, winattr::READONLY);
        winattr::change(&ini, winattr::HIDDEN | winattr::SYSTEM, 0);
        // Windows 只对带"只读"标记的文件夹读 desktop.ini；这个标记对文件夹没有别的作用
        winattr::change(dir, winattr::READONLY, 0);
    } else {
        if ini.is_file() {
            fs::remove_file(&ini).map_err(|e| e.to_string())?;
        }
        if icon.is_file() {
            fs::remove_file(&icon).map_err(|e| e.to_string())?;
        }
        winattr::change(dir, 0, winattr::READONLY);
    }
    winattr::refresh(dir);
    Ok(())
}

#[tauri::command]
fn set_folder_icon(app: AppHandle, path: String, on: bool) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("文件夹图标目前只支持 Windows".into());
    }
    // 去掉 \\?\ 前缀：资源管理器的刷新通知不认这种写法
    let dir = PathBuf::from(plain(&check_project(Path::new(&path))?));
    apply_folder_icon(&dir, folder_ico(&app).as_deref(), on)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_guard() {
        let root = tmp("del");
        let proj = root.join("p1");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("project.json"), "{}").unwrap();
        let plain = root.join("not_a_project");
        fs::create_dir_all(&plain).unwrap();
        assert!(check_deletable(&root, &proj).is_ok());
        assert!(check_deletable(&root, &root).is_err());
        assert!(check_deletable(&root, &plain).is_err());
        assert!(check_deletable(&proj, &root).is_err());
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tdl-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn create_then_scan_then_save() {
        let root = tmp("flow");
        let tpl = root.join("tpl.toe");
        fs::write(&tpl, b"fake toe").unwrap();
        let r = root.to_string_lossy().to_string();

        let c = create_project_at(r.clone(), "2026-11-17_粒子雨".into(), "粒子雨".into(), "2026-11-17".into(), Some(tpl.clone()), None, vec![], false).unwrap();
        let dir = PathBuf::from(&c.path);
        for sub in SUBDIRS {
            assert!(dir.join(sub).is_dir(), "缺少 {}", sub);
        }
        assert!(dir.join("粒子雨.toe").is_file());
        assert_eq!(c.toe.is_some(), true);

        // 同名再建一次应当报错
        assert!(create_project_at(r.clone(), "2026-11-17_粒子雨".into(), "粒子雨".into(), "x".into(), None, None, vec![], false).is_err());
        // 非法字符
        assert!(create_project_at(r.clone(), "a:b".into(), "a:b".into(), "x".into(), None, None, vec![], false).is_err());

        // 旧工程：只有 .toe（带版本号）和 thumb.jpg，没有 project.json
        let old = root.join("旧工程");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("demo.12.toe"), b"x").unwrap();
        fs::write(old.join("thumb.jpg"), b"x").unwrap();
        // 不是工程的文件夹 + Backup 文件夹应被跳过
        fs::create_dir_all(root.join("随便的文件夹")).unwrap();
        fs::create_dir_all(root.join("Backup")).unwrap();
        fs::write(root.join("Backup").join("x.toe"), b"x").unwrap();

        // 只认两层：01_商单 是分类，客户A 是工程；它的 .toe 在更深的 海报 里，不单独拆出来
        let nested = root.join("01_商单").join("客户A").join("海报");
        fs::create_dir_all(nested.join("Backup")).unwrap();
        fs::write(nested.join("poster.toe"), b"x").unwrap();
        fs::write(nested.join("Backup").join("poster.3.toe"), b"x").unwrap();

        let list = scan_projects(r.clone(), None).unwrap();
        assert_eq!(list.len(), 3, "应只扫到 3 个工程");
        assert!(list.iter().all(|p| p.folder != "海报"), "更深的文件夹不单独算工程");
        let n = list.iter().find(|p| p.folder == "客户A").unwrap();
        assert_eq!(n.group, "01_商单");
        assert_eq!(n.name, "客户A");
        assert!(n.toe.as_ref().unwrap().ends_with("poster.toe"), ".toe 在里面一层也能找到并打开");
        assert!(check_deletable(&root, Path::new(&n.path)).is_ok(), ".toe 不在第一层的工程也能移动、删除");
        let new = list.iter().find(|p| p.folder == "2026-11-17_粒子雨").unwrap();
        assert_eq!(new.name, "粒子雨");
        assert_eq!(new.meta["status"], "进行中");
        let o = list.iter().find(|p| p.folder == "旧工程").unwrap();
        assert_eq!(o.name, "旧工程");
        assert!(o.thumb.is_some());
        assert!(!o.has_meta);

        // 保存：合并字段，保留未知字段
        let pj = dir.join("project.json");
        let mut v: Value = serde_json::from_str(&fs::read_to_string(&pj).unwrap()).unwrap();
        v["custom"] = json!("保留我");
        fs::write(&pj, v.to_string()).unwrap();
        save_meta(c.path.clone(), json!({"status": "已交付", "tags": ["粒子", "GPU"]})).unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(&pj).unwrap()).unwrap();
        assert_eq!(v["status"], "已交付");
        assert_eq!(v["tags"][1], "GPU");
        assert_eq!(v["custom"], "保留我");
        assert_eq!(v["name"], "粒子雨");

        // 旧工程也能写入标注
        save_meta(o.path.clone(), json!({"status": "搁置"})).unwrap();
        assert!(old.join("project.json").is_file());

        let _ = fs::remove_dir_all(&root);
    }

    /// 只读：扫一遍真实的工程目录，打印结果（cargo test real_root -- --ignored --nocapture）
    #[test]
    #[ignore]
    fn real_root() {
        let r = std::env::var("TDL_ROOT").unwrap_or(r"F:\Projects\Touchdesigner Projects".into());
        let t = std::time::Instant::now();
        let list = scan_projects(r, None).unwrap();
        println!("找到 {} 个工程，用时 {:?}", list.len(), t.elapsed());
        let mut groups: std::collections::BTreeMap<String, usize> = Default::default();
        for p in &list {
            *groups.entry(p.group.split('/').next().unwrap_or("").to_string()).or_default() += 1;
        }
        println!("{:?}", groups);
        for p in list.iter() {
            println!("  [{}] {} | {} | {} | {:?}", p.confidence, p.group, p.folder, p.name, p.toe_bases);
        }
    }

    #[test]
    fn detect_rules() {
        let root = tmp("rules");
        let mk = |rel: &str, files: &[&str]| {
            let d = root.join(rel);
            fs::create_dir_all(&d).unwrap();
            for f in files {
                fs::write(d.join(f), b"x").unwrap();
            }
            d
        };
        // TD 原生结构 → likely；两个不同名字的 .toe；崩溃存档不算主文件
        mk("02_案例/Skin", &["Skin.toe", "Skin.21.toe", "Other.3.toe", "Other.5.toe", "CrashAutoSave.Skin.4.toe"]);
        mk("02_案例/Skin/Backup", &["Skin.1.toe"]);
        mk("02_案例/Skin/Audio", &[]);
        // 工程里附带的 .toe 不算工程
        mk("02_案例/Skin/Mediapipe_release", &["MediaPipe.toe"]);
        // 工程里更深的文件夹，哪怕有 project.json 也不单独算
        mk("02_案例/Skin/2026-11-17_Sub", &["project.json", "sub.toe"]);
        // 零散 .toe → maybe
        mk("loose", &["a.toe"]);
        // 只剩 Backup → 不算
        mk("only_backup/Backup", &["x.3.toe"]);
        // Mac 影子文件 / __MACOSX / _开头 → 都不算
        mk("mac_zip", &["._ghost.toe"]);
        mk("mac_zip/__MACOSX/inner", &["._a.toe"]);
        mk("_to_delete/old", &["old.toe"]);
        // 库文件夹和别的分类一样对待
        mk("00_Library/Mediapipe", &["m.toe"]);
        mk("03_x/Library", &["lib.toe"]);
        // 分类文件夹里的工程，.toe 分散在几个子文件夹：算一个工程，用最近保存的那个子文件夹
        mk("03_x/多版本/v1", &["a.toe"]);
        mk("03_x/多版本/v2/工程文件", &["b.toe", "b.2.toe"]);
        mk("03_x/空文件夹/只有素材", &["x.png"]);
        // 分类文件夹自己带着 project.json（被误导入过）：仍然是分类，里面的文件夹才是工程
        mk("04_误导入", &["project.json"]);
        mk("04_误导入/甲", &["a.toe"]);

        let list = scan_projects(root.to_string_lossy().to_string(), None).unwrap();
        let names: Vec<_> = list.iter().map(|p| format!("{}|{}", p.group, p.folder)).collect();
        assert_eq!(list.len(), 6, "{:?}", names);
        assert!(list.iter().all(|p| p.folder != "04_误导入"), "{:?}", names);
        assert_eq!(list.iter().find(|p| p.folder == "甲").unwrap().group, "04_误导入");
        assert_eq!(list.iter().find(|p| p.folder == "Mediapipe").unwrap().group, "00_Library");
        assert!(list.iter().all(|p| p.folder != "2026-11-17_Sub" && p.folder != "v1" && p.folder != "v2" && p.folder != "空文件夹"), "{:?}", names);
        let multi = list.iter().find(|p| p.folder == "多版本").unwrap();
        assert_eq!(multi.group, "03_x");
        assert!(multi.toe.is_some());
        let skin = list.iter().find(|p| p.folder == "Skin").unwrap();
        assert_eq!(skin.confidence, "likely");
        assert_eq!(skin.toe_bases, vec!["Other".to_string(), "Skin".to_string()]);
        assert!(!skin.toe.as_ref().unwrap().contains("CrashAutoSave"));
        assert!(skin.earliest > 0);
        assert_eq!(list.iter().find(|p| p.folder == "loose").unwrap().confidence, "maybe");
        assert!(list.iter().any(|p| p.folder == "Library"));

        // mainToe 指定主文件
        save_meta(skin.path.clone(), json!({"mainToe": "Other"})).unwrap();
        let list = scan_projects(root.to_string_lossy().to_string(), None).unwrap();
        let skin = list.iter().find(|p| p.folder == "Skin").unwrap();
        assert!(skin.toe.as_ref().unwrap().contains("Other."));

        assert_eq!(system_junk("__MACOSX"), Some("macOS"));
        assert_eq!(system_junk("._x.toe"), Some("macOS"));
        assert_eq!(system_junk("$RECYCLE.BIN"), Some("Windows"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn move_rules() {
        let root = tmp("move");
        let r = root.to_string_lossy().to_string();
        let p = root.join("proj");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("a.toe"), b"x").unwrap();
        // 移进新建的分类文件夹
        let np = move_project(r.clone(), p.to_string_lossy().into(), "01_商单/客户A".into(), None).unwrap();
        assert!(root.join("01_商单/客户A/proj/a.toe").is_file());
        assert!(!p.exists());
        assert!(!np.starts_with(r"\\?\"));
        // 不能移进自己
        assert!(move_project(r.clone(), np.clone(), "01_商单/客户A/proj/inner".into(), None).is_err());
        // 同名冲突
        fs::create_dir_all(root.join("proj")).unwrap();
        assert!(move_project(r.clone(), np.clone(), "".into(), None).is_err());
        // 非法名字
        assert!(move_project(r.clone(), np.clone(), "../外面".into(), None).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn import_rules() {
        let base = tmp("imp");
        let root = base.join("root");
        let outside = base.join("outside");
        let mk = |d: PathBuf, files: &[&str]| {
            fs::create_dir_all(&d).unwrap();
            for f in files {
                fs::write(d.join(f), b"x").unwrap();
            }
            d
        };
        mk(root.join("inroot"), &["a.toe"]);
        let single = mk(outside.join("单个工程"), &["s.toe", "s.3.toe"]);
        mk(outside.join("一批").join("甲"), &["j.toe"]);
        mk(outside.join("一批").join("乙"), &["project.json"]);
        mk(outside.join("一批").join("不是工程"), &["readme.txt"]);
        mk(outside.join("一批").join("Backup"), &["old.1.toe"]);
        let r = root.to_string_lossy().to_string();
        let s = |p: &Path| p.to_string_lossy().to_string();

        // 选的文件夹本身是工程
        let one = inspect_import(r.clone(), s(&single)).unwrap();
        assert!(!one.inside_root);
        assert_eq!(one.projects.len(), 1);
        assert!(one.projects[0].external);
        assert_eq!(one.projects[0].group, "outside", "单个导入：按上一级文件夹分组");
        assert_eq!(one.projects[0].path, s(&single));
        // 拖进来的是 .toe 文件：看它所在的文件夹
        assert_eq!(inspect_import(r.clone(), s(&single.join("s.toe"))).unwrap().projects.len(), 1);
        // 选的是上级文件夹：列出里面的工程，跳过 Backup 和不是工程的
        let many = inspect_import(r.clone(), s(&outside.join("一批"))).unwrap();
        let mut names: Vec<_> = many.projects.iter().map(|p| p.folder.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["乙".to_string(), "甲".to_string()]);
        assert!(many.projects.iter().all(|p| p.group == "一批" && p.external), "直接放在所选文件夹里的：按这个文件夹分组");
        // 已经在根目录里的不用导入
        assert!(inspect_import(r.clone(), s(&root.join("inroot"))).unwrap().inside_root);
        // 选更上一级：只认它直属的文件夹，每个算一个工程；根目录本身不算
        let up = inspect_import(r.clone(), s(&base)).unwrap();
        let ups: Vec<_> = up.projects.iter().map(|p| p.folder.clone()).collect();
        assert_eq!(ups, vec!["outside".to_string()], "根目录不该出现，更深的文件夹不单独拆");
        let grand = inspect_import(r.clone(), s(&outside)).unwrap();
        let mut gs: Vec<_> = grand.projects.iter().map(|p| format!("{}|{}", p.group, p.folder)).collect();
        gs.sort();
        assert_eq!(gs, vec!["outside|一批".to_string(), "outside|单个工程".to_string()]);
        // 检查没有写任何东西
        assert!(!single.join("project.json").exists());

        // 扫描：根目录 + 登记的外部工程；找不到的和根目录里的都不重复
        let extra = vec![s(&single), s(&outside.join("不存在")), s(&root.join("inroot"))];
        let list = scan_projects(r.clone(), Some(extra)).unwrap();
        assert_eq!(list.len(), 2);
        let e = list.iter().find(|p| p.external).unwrap();
        assert_eq!(e.path, s(&single));
        assert_eq!(e.group, EXT_GROUP);
        assert!(!list.iter().find(|p| p.folder == "inroot").unwrap().external);

        // 导入的工程不能当根目录里的删；移进分类文件夹要带 external
        assert!(check_deletable(&root, &single).is_err());
        assert!(move_project(r.clone(), s(&single), "01_商单".into(), None).is_err());
        let np = move_project(r.clone(), s(&single), "01_商单".into(), Some(true)).unwrap();
        assert!(root.join("01_商单").join("单个工程").join("s.toe").is_file());
        assert!(!single.exists());
        assert!(inside(&root, Path::new(&np)));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn folder_icon_files() {
        let d = tmp("ico");
        let ico = d.join("src.ico");
        fs::write(&ico, b"ico").unwrap();
        let p = d.join("proj");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("a.toe"), b"x").unwrap();
        assert!(!has_folder_icon(&p));
        // 没有图标文件时报错，不留半截
        assert!(apply_folder_icon(&p, None, true).is_err());
        assert!(!p.join("desktop.ini").exists());
        apply_folder_icon(&p, Some(&ico), true).unwrap();
        assert!(has_folder_icon(&p));
        assert_eq!(fs::read_to_string(p.join("desktop.ini")).unwrap(), ICON_INI);
        // 再设一次（覆盖带隐藏属性的文件）不出错
        apply_folder_icon(&p, Some(&ico), true).unwrap();
        apply_folder_icon(&p, None, false).unwrap();
        assert!(!p.join("desktop.ini").exists() && !p.join(ICON_FILE).exists());
        assert!(p.join("a.toe").is_file());
        // 别人写的 desktop.ini 不动
        fs::write(p.join("desktop.ini"), "[.ShellClassInfo]\r\nIconResource=other.ico,0\r\n").unwrap();
        assert!(apply_folder_icon(&p, Some(&ico), true).is_err());
        assert!(apply_folder_icon(&p, None, false).is_err());
        assert!(fs::read_to_string(p.join("desktop.ini")).unwrap().contains("other.ico"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn custom_logo_files() {
        let d = tmp("logo").join("custom");
        let png = [0x89, b'P', b'N', b'G', 1, 2, 3];
        let mut ico = vec![0u8, 0, 1, 0, 1, 0];
        ico.extend_from_slice(&[0u8; 20]);
        // 内容不对的不存，也不建文件夹
        assert!(write_custom_logo(&d, b"not png", &ico).is_err());
        assert!(write_custom_logo(&d, &png, b"not ico").is_err());
        assert!(!d.exists());
        let p = write_custom_logo(&d, &png, &ico).unwrap();
        assert_eq!(fs::read(&p).unwrap(), png);
        assert_eq!(fs::read(d.join("folder.ico")).unwrap(), ico);
        // 只读图片
        let txt = d.join("a.txt");
        fs::write(&txt, b"x").unwrap();
        assert!(read_image(txt.to_string_lossy().to_string()).is_err());
        assert_eq!(read_image(p.to_string_lossy().to_string()).unwrap(), png);
        let _ = fs::remove_dir_all(d.parent().unwrap());
    }

    #[test]
    fn custom_templates() {
        let base = tmp("tpl");
        let (root, tdir) = (base.join("root"), base.join("templates"));
        fs::create_dir_all(&root).unwrap();
        prepare_templates_dir(&tdir).unwrap();
        assert!(read_templates(&tdir).is_empty(), "说明文件不是模板");
        let builtin = base.join("builtin.toe");
        fs::write(&builtin, b"builtin").unwrap();

        // 手工做的模板：有子文件夹、文件、两个版本的 .toe，还有不该带走的东西
        let t = tdir.join("演出");
        for d in ["素材/视频", "灯光", "Backup"] {
            fs::create_dir_all(t.join(d)).unwrap();
        }
        fs::write(t.join("素材/视频/开场.txt"), b"v").unwrap();
        fs::write(t.join("清单.md"), b"m").unwrap();
        fs::write(t.join("show.toe"), b"old").unwrap();
        fs::write(t.join("show.2.toe"), b"new").unwrap();
        fs::write(t.join("project.json"), b"{}").unwrap();
        fs::write(t.join("thumb.jpg"), b"x").unwrap();
        fs::write(t.join("Backup/show.1.toe"), b"x").unwrap();
        // 让 show.2.toe 是最近保存的
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(t.join("show.2.toe"), b"new").unwrap();
        fs::create_dir_all(tdir.join("空模板/a/b")).unwrap();

        let list = read_templates(&tdir);
        assert_eq!(list.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), vec!["演出", "空模板"]);
        assert!(list[0].has_toe && !list[1].has_toe);
        assert_eq!(list[0].entries, vec!["灯光/", "素材/", "清单.md"]);

        let r = root.to_string_lossy().to_string();
        let c = create_project_at(r.clone(), "2026_跨年".into(), "跨年".into(), "2026-12-31".into(), Some(builtin.clone()), Some(t.clone()), vec![], false).unwrap();
        let p = PathBuf::from(&c.path);
        assert_eq!(fs::read(p.join("跨年.toe")).unwrap(), b"new", "用最近保存的 .toe 并改名");
        assert!(p.join("素材/视频/开场.txt").is_file() && p.join("灯光").is_dir() && p.join("清单.md").is_file());
        assert!(!p.join("show.toe").exists() && !p.join("show.2.toe").exists() && !p.join("Backup").exists() && !p.join("thumb.jpg").exists());
        let meta: Value = serde_json::from_str(&fs::read_to_string(p.join("project.json")).unwrap()).unwrap();
        assert_eq!(meta["name"], "跨年");
        assert!(!p.join("assets").exists(), "自己的模板不加默认的子文件夹");

        // 模板里没有 .toe：用自带的
        let c2 = create_project_at(r.clone(), "空的".into(), "空的".into(), "x".into(), Some(builtin.clone()), Some(tdir.join("空模板")), vec![], false).unwrap();
        assert_eq!(fs::read(PathBuf::from(&c2.path).join("空的.toe")).unwrap(), b"builtin");
        assert!(PathBuf::from(&c2.path).join("a/b").is_dir());

        // 把工程存成模板：只有文件夹结构和 .toe
        save_template_at(&tdir, &p, "跨年模板").unwrap();
        let s = tdir.join("跨年模板");
        assert!(s.join("素材/视频").is_dir() && s.join("灯光").is_dir());
        assert!(!s.join("素材/视频/开场.txt").exists() && !s.join("清单.md").exists() && !s.join("project.json").exists());
        assert_eq!(fs::read(s.join("template.toe")).unwrap(), b"new");
        assert!(save_template_at(&tdir, &p, "跨年模板").is_err(), "重名");
        assert!(save_template_at(&tdir, &p, "../外面").is_err() && save_template_at(&tdir, &p, "_x").is_err() && save_template_at(&tdir, &p, "  ").is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn preview_images() {
        let base = tmp("prev");
        let p = base.join("旧工程");
        let big = vec![7u8; 20 * 1024];
        let mk = |rel: &str, data: &[u8]| {
            let f = p.join(rel);
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(&f, data).unwrap();
            f
        };
        mk("old.toe", b"x");
        // 没有任何图：没有预览
        assert_eq!(resolve_thumb(&p, None), (None, false));
        mk("icon.png", b"tiny"); // 太小，不算
        mk("assets/tex/wood.png", &big); // 素材贴图，减分
        mk("a/b/c/deep.png", &big); // 太深，不找
        mk("Backup/old.png", &big); // Backup 不找
        let (got, auto) = resolve_thumb(&p, None);
        assert!(auto && got.unwrap().ends_with("wood.png"), "只有贴图时也用它，总比没有强");
        let out = mk("export/final.png", &big);
        assert_eq!(resolve_thumb(&p, None), (Some(out), true), "导出文件夹里的优先于素材");
        let shot = mk("TDMovieOut.0.jpg", &big);
        assert_eq!(resolve_thumb(&p, None), (Some(shot), true), "TD 导出的截图最优先");
        // 扫描结果里带上
        let pr = read_project(&p, "旧工程", "").unwrap();
        assert!(pr.thumb_auto && pr.thumb.unwrap().ends_with("TDMovieOut.0.jpg"));
        assert!(!p.join("project.json").exists() && !p.join("thumb.jpg").exists(), "自动找图不往文件夹里写东西");

        // 用户说不用：不再自动找
        set_thumb_at(&p, None).unwrap();
        assert_eq!(resolve_thumb(&p, Some("-")), (None, false));
        assert!(read_project(&p, "旧工程", "").unwrap().thumb.is_none());
        // 用户选文件夹里的一张：记相对位置
        set_thumb_at(&p, Some(&p.join("assets/tex/wood.png").to_string_lossy())).unwrap();
        let pr = read_project(&p, "旧工程", "").unwrap();
        assert_eq!(pr.meta["thumbFile"], "assets/tex/wood.png");
        assert!(!pr.thumb_auto && pr.thumb.unwrap().ends_with("wood.png"));
        // 选文件夹外面的一张：复制进来
        let outside = base.join("别处.PNG");
        fs::write(&outside, &big).unwrap();
        set_thumb_at(&p, Some(&outside.to_string_lossy())).unwrap();
        assert!(p.join("thumb-custom.png").is_file());
        assert_eq!(read_project(&p, "旧工程", "").unwrap().meta["thumbFile"], "thumb-custom.png");
        // 不是图片 / 想指到文件夹外面：不认
        assert!(set_thumb_at(&p, Some(&p.join("old.toe").to_string_lossy())).is_err());
        assert_eq!(resolve_thumb(&p, Some("../别处.PNG")).1, true, "越界的 thumbFile 当没写，回到自动找");
        // 恢复自动；之后 TD 存了 thumb.jpg 就用它
        set_thumb_at(&p, Some("")).unwrap();
        assert!(read_project(&p, "旧工程", "").unwrap().thumb_auto);
        let tj = mk("thumb.jpg", &big);
        assert_eq!(resolve_thumb(&p, Some("")), (Some(tj), false));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn user_theme_files() {
        let d = tmp("themes");
        assert!(read_user_themes(&d.join("没有这个文件夹")).is_empty());
        prepare_themes_dir(&d).unwrap();
        assert!(d.join("主题制作规范.md").is_file());
        // 模板以 _ 开头，不算主题；它本身是合法的 JSON
        assert!(read_user_themes(&d).is_empty());
        let tpl: Value = serde_json::from_str(&fs::read_to_string(d.join("_template.json")).unwrap()).unwrap();
        assert_eq!(tpl["format"], 1);
        assert!(tpl["light"]["ink"].is_string() && tpl["dark"]["onInk"].is_string());
        // 用户改过的模板不覆盖
        fs::write(d.join("_template.json"), "mine").unwrap();
        prepare_themes_dir(&d).unwrap();
        assert_eq!(fs::read_to_string(d.join("_template.json")).unwrap(), "mine");

        fs::write(d.join("a.json"), "\u{feff}{\"format\":1,\"id\":\"a\"}").unwrap();
        fs::write(d.join("b.json"), "{ 坏的").unwrap();
        fs::write(d.join("note.txt"), "x").unwrap();
        let list = read_user_themes(&d);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].file, "a.json");
        assert_eq!(list[0].data["id"], "a");
        assert!(list[0].error.is_none());
        assert!(list[1].error.as_ref().unwrap().contains("JSON"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn version_groups() {
        let d = tmp("ver");
        for f in ["StreamDiffusionTD_0.3.0.toe", "StreamDiffusionTD_0.3.0.6.toe", "ue5.1.toe", "ue5.13.toe", "a.toe", "a.2.toe"] {
            fs::write(d.join(f), b"x").unwrap();
        }
        let mut b: Vec<String> = toes_in(&d).into_iter().map(|t| t.base).collect();
        b.sort();
        b.dedup();
        assert_eq!(b, vec!["StreamDiffusionTD_0.3.0", "a", "ue5"]);
        assert_eq!(folder_title("2026-11-17_粒子雨"), "粒子雨");
        assert_eq!(folder_title("四文物"), "四文物");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn version_suffix() {
        assert_eq!(strip_version("粒子雨.12"), "粒子雨");
        assert_eq!(strip_version("v1.final"), "v1.final");
        assert_eq!(strip_version("plain"), "plain");
    }
    #[test]
    fn components_and_case() {
        let base = tmp("comps");
        let root = base.join("root");
        fs::create_dir_all(&root).unwrap();
        let lib = base.join("lib");
        fs::create_dir_all(lib.join("Mediapipe/toxes")).unwrap();
        fs::write(lib.join("TDMCP.tox"), b"t").unwrap();
        fs::write(lib.join("Mediapipe/MediaPipe.tox"), b"m").unwrap();
        fs::write(lib.join("Mediapipe/toxes/hand.tox"), b"h").unwrap();
        fs::write(lib.join("Mediapipe/desktop.ini"), b"x").unwrap();
        fs::write(lib.join("Mediapipe/project.json"), b"{}").unwrap();
        let comps = vec![
            ("TDMCP".to_string(), lib.join("TDMCP.tox"), vec![]),
            ("Mediapipe".to_string(), lib.join("Mediapipe"), vec!["MediaPipe.tox".to_string(), "toxes\\hand.tox".to_string(), "没有.tox".to_string(), "../x.tox".to_string()]),
        ];
        let r = root.to_string_lossy().to_string();
        let c = create_project_at(r.clone(), "CASE15_测试".into(), "测试".into(), "2026-11-26".into(), None, None, comps, true).unwrap();
        let dir = PathBuf::from(&c.path);
        assert!(dir.join("tox/TDMCP.tox").is_file());
        assert!(dir.join("tox/Mediapipe/toxes/hand.tox").is_file());
        assert!(!dir.join("tox/Mediapipe/desktop.ini").exists() && !dir.join("tox/Mediapipe/project.json").exists());
        let meta = read_meta(&dir).unwrap();
        let list = meta["components"].as_array().unwrap();
        assert_eq!(list[0]["files"], json!(["tox/TDMCP.tox"]));
        assert_eq!(list[1]["files"], json!(["tox/Mediapipe/MediaPipe.tox", "tox/Mediapipe/toxes/hand.tox"]));
        assert_eq!(list[1]["loaded"], json!(false));
        // 案例工程：demo 文件夹和说明
        assert!(dir.join("demo").is_dir());
        assert!(fs::read_to_string(dir.join("README.md")).unwrap().contains("测试"));
        // 组件文件不见了 → 报错，不留半成品
        let bad = vec![("没了".to_string(), lib.join("nope.tox"), vec![])];
        assert!(create_project_at(r.clone(), "坏的".into(), "坏的".into(), "x".into(), None, None, bad, false).is_err());
        assert!(!root.join("坏的").exists());
        // 规范检查
        let chk = check_case(c.path.clone(), None);
        let get = |l: &str| chk.iter().find(|x| x.label == l).unwrap().ok;
        assert!(get("有说明文档"));
        assert!(!get("有演示视频") && !get("工程文件在第一层，只有一个") && !get("标注完整") && !get("素材在工程里"), "assets 是空的不算");
        fs::write(dir.join("assets/images/a.png"), b"p").unwrap();
        fs::write(dir.join("demo/演示.mp4"), b"v").unwrap();
        fs::write(dir.join("测试.toe"), b"t").unwrap();
        save_meta(c.path.clone(), json!({"tags": ["手势"], "note": "一句话"})).unwrap();
        let chk = check_case(c.path.clone(), None);
        assert!(chk.iter().filter(|x| x.label != "有预览图").all(|x| x.ok));
        // 定制工程的清单；第一层多一个别的 .toe 就不算「只有一个」
        let ord = check_case(c.path.clone(), Some("order".into()));
        let og = |l: &str| ord.iter().find(|x| x.label == l).unwrap().ok;
        assert!(og("工程文件在第一层，只有一个") && !og("标注完整") && !og("deliver 里留了压缩包"));
        assert!(ord.iter().all(|x| x.label != "有演示视频"));
        save_meta(c.path.clone(), json!({"slug": "test", "version": "1.0", "tdVersion": "2025.1"})).unwrap();
        fs::create_dir_all(dir.join("deliver")).unwrap();
        fs::write(dir.join("deliver/test_v1.0_20261126.zip"), b"z").unwrap();
        fs::write(dir.join("实验.toe"), b"t").unwrap();
        fs::write(dir.join("测试.3.toe"), b"t").unwrap();
        let ord = check_case(c.path.clone(), Some("order".into()));
        let og = |l: &str| ord.iter().find(|x| x.label == l).unwrap().ok;
        assert!(og("标注完整") && og("deliver 里留了压缩包") && !og("工程文件在第一层，只有一个"));
        // save_meta 不会把组件清单冲掉
        assert!(read_meta(&dir).unwrap()["components"].is_array());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn watermark_scan() {
        let root = tmp("wm");
        fs::create_dir_all(&root).unwrap();
        let r = root.to_string_lossy().to_string();
        let s = wm_scan(r.clone(), false).unwrap();
        assert!(!s.exists && s.items.is_empty() && !root.join("案例视频").exists());
        let s = wm_scan(r.clone(), true).unwrap();
        assert!(s.exists && root.join("案例视频/无水印").is_dir() && root.join("案例视频/水印版").is_dir());
        let src = root.join("案例视频/无水印");
        fs::create_dir_all(src.join("竖版")).unwrap();
        fs::write(src.join("CASE01_手势寻宝_握拳收集.mov"), b"v").unwrap();
        fs::write(src.join("竖版/CASE02_晕染.mp4"), b"v").unwrap();
        fs::write(src.join("说明.txt"), b"x").unwrap();
        fs::create_dir_all(src.join("_草稿")).unwrap();
        fs::write(src.join("_草稿/x.mp4"), b"v").unwrap();
        let s = wm_scan(r.clone(), false).unwrap();
        let rels: Vec<_> = s.items.iter().map(|i| i.rel.clone()).collect();
        assert_eq!(rels, vec!["CASE01_手势寻宝_握拳收集.mov".to_string(), "竖版/CASE02_晕染.mp4".to_string()]);
        assert!(s.items.iter().all(|i| !i.done));
        assert!(s.items[0].out.replace('\\', "/").ends_with("案例视频/水印版/CASE01_手势寻宝_握拳收集.mp4"));
        // 水印版已经有了 → 算同步好
        fs::create_dir_all(root.join("案例视频/水印版/竖版")).unwrap();
        fs::write(root.join("案例视频/水印版/竖版/CASE02_晕染.mp4"), b"o").unwrap();
        let s = wm_scan(r.clone(), false).unwrap();
        assert!(!s.items[0].done && s.items[1].done);
        assert_eq!(parse_size("  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709), 1920x1080 [SAR 1:1 DAR 16:9], 8000 kb/s"), Some((1920, 1080)));
        assert_eq!(parse_size("Stream #0:0: Video: prores, yuv422p10le, 1080x1920, 30 fps\nStream #0:1: Audio: aac"), Some((1080, 1920)));
        assert_eq!(parse_size("Stream #0:1: Audio: aac, 48000 Hz"), None);
        let _ = fs::remove_dir_all(&root);
    }
    #[test]
    fn pack_and_web() {
        let root = tmp("pack");
        let dir = root.join("02_案例工程/CASE06_Pixel Gem");
        for d in ["assets/images", "tox", "scripts", "demo", "export", "Backup", "docs", "deliver"] {
            fs::create_dir_all(dir.join(d)).unwrap();
        }
        for (f, c) in [("PixelGem.toe", "t"), ("PixelGem.3.toe", "old"), ("Other.toe", "o"), ("README.md", "r"), ("thumb.jpg", "j"), ("assets/images/a.png", "p"), ("tox/x.tox", "x"), ("demo/raw.mp4", "v"), ("export/e.mov", "e"), ("Backup/b.toe", "b"), ("docs/报价.txt", "d"), ("assets/project.json", "{}")] {
            fs::write(dir.join(f), c).unwrap();
        }
        fs::write(dir.join("project.json"), r#"{"name":"像素宝石","caseId":"case06","slug":"pixel-gem","version":"1.0","mainToe":"PixelGem"}"#).unwrap();
        let names = |p: &Path| -> Vec<String> {
            let mut z = zip::ZipArchive::new(fs::File::open(p).unwrap()).unwrap();
            let mut v: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).collect();
            v.sort();
            v
        };
        // 定制单打包
        let p = pack_order(dir.to_string_lossy().to_string(), "2026-11-26".into()).unwrap();
        assert!(p.path.replace('\\', "/").ends_with("deliver/pixel-gem_v1.0_20261126.zip"));
        let n = names(Path::new(&p.path));
        let top = "pixel-gem_v1.0_20261126";
        assert_eq!(n, vec![format!("{top}/PixelGem.toe"), format!("{top}/README.md"), format!("{top}/assets/images/a.png"), format!("{top}/tox/x.tox")], "{:?}", n);
        // 官网上传包：没有水印版视频 → 不生成
        let r = root.to_string_lossy().to_string();
        let e = web_pack(r.clone(), dir.to_string_lossy().to_string()).err().unwrap();
        assert!(e.contains("没有这个案例的视频"), "{}", e);
        assert!(!root.join("_官网上传").exists());
        // 放一个假的 faststart H.264 mp4 + 封面，再放一个名字不对的和一个无水印的
        let mut mp4: Vec<u8> = Vec::new();
        mp4.extend(&16u32.to_be_bytes()); mp4.extend(b"ftypisom\0\0\0\0isom"[..12].iter());
        let moov_body = b"....avc1....";
        mp4.extend(&((8 + moov_body.len()) as u32).to_be_bytes()); mp4.extend(b"moov"); mp4.extend(moov_body);
        mp4.extend(&12u32.to_be_bytes()); mp4.extend(b"mdat"); mp4.extend(b"data");
        let wm = root.join("案例视频/水印版");
        fs::create_dir_all(wm.join("竖版")).unwrap();
        fs::create_dir_all(root.join("案例视频/无水印")).unwrap();
        fs::write(wm.join("case06_pixel-gem_16x9.mp4"), &mp4).unwrap();
        fs::write(wm.join("case06_pixel-gem_16x9_poster.jpg"), b"j").unwrap();
        fs::write(wm.join("竖版/case06_pixel-gem_9x16_v2.mp4"), &mp4).unwrap();
        fs::write(wm.join("case07_other_16x9.mp4"), &mp4).unwrap();
        fs::write(wm.join("case06_pixel-gem_1x1.mp4"), &mp4).unwrap();
        fs::write(root.join("案例视频/无水印/case06_pixel-gem_4x3.mp4"), &mp4).unwrap();
        let e = web_pack(r.clone(), dir.to_string_lossy().to_string()).err().unwrap();
        assert!(e.contains("9x16_v2.mp4 还没有封面") && !e.contains("16x9.mp4 还没有封面"), "{}", e);
        fs::write(wm.join("竖版/case06_pixel-gem_9x16_v2_poster.jpg"), b"j").unwrap();
        let w = web_pack(r.clone(), dir.to_string_lossy().to_string()).unwrap();
        let mut got: Vec<String> = fs::read_dir(&w.dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        got.sort();
        assert_eq!(got, vec!["case06-pixel-gem_v1.0.zip", "case06_pixel-gem_16x9.mp4", "case06_pixel-gem_16x9_poster.jpg", "case06_pixel-gem_9x16_v2.mp4", "case06_pixel-gem_9x16_v2_poster.jpg", "project.json"], "{:?}", got);
        assert!(w.dir.replace('\\', "/").ends_with("_官网上传/case06-pixel-gem"));
        let n = names(&Path::new(&w.dir).join("case06-pixel-gem_v1.0.zip"));
        let top = "case06-pixel-gem_v1.0";
        assert_eq!(n, vec![format!("{top}/PixelGem.toe"), format!("{top}/README.md"), format!("{top}/assets/images/a.png"), format!("{top}/demo/raw.mp4"), format!("{top}/thumb.jpg"), format!("{top}/tox/x.tox")], "{:?}", n);
        // 官网上传文件夹不会被当成工程扫进来
        let list = scan_projects(r.clone(), None).unwrap();
        assert!(list.iter().all(|p| !p.path.contains("_官网上传")));
        // 文件头检查
        let bad = root.join("x.mp4");
        let mut late: Vec<u8> = Vec::new();
        late.extend(&12u32.to_be_bytes()); late.extend(b"mdat"); late.extend(b"data");
        late.extend(&20u32.to_be_bytes()); late.extend(b"moov"); late.extend(b"....avc1....");
        fs::write(&bad, &late).unwrap();
        assert!(mp4_web_ok(&bad).unwrap_err().contains("faststart"));
        let mut hevc: Vec<u8> = Vec::new();
        hevc.extend(&20u32.to_be_bytes()); hevc.extend(b"moov"); hevc.extend(b"....hvc1....");
        fs::write(&bad, &hevc).unwrap();
        assert!(mp4_web_ok(&bad).unwrap_err().contains("H.264"));
        assert!(web_video_name("case06_pixel-gem_16x9", "case06", "pixel-gem") && web_video_name("case06_pixel-gem_3x4_v12", "case06", "pixel-gem"));
        assert!(!web_video_name("case06_pixel-gem_16x9_final", "case06", "pixel-gem") && !web_video_name("case06_pixel-gem", "case06", "pixel-gem") && !web_video_name("case060_pixel-gem_16x9", "case06", "pixel-gem"));
        assert_eq!(parse_duration("  Duration: 00:01:02.50, start: 0.000000, bitrate: 4000 kb/s"), Some(62.5));
        let _ = fs::remove_dir_all(&root);
    }
}

/* ================= 案例工程规范检查 ================= */

const VIDEO_EXTS: [&str; 6] = ["mp4", "mov", "mkv", "avi", "webm", "m4v"];

fn is_video(p: &Path) -> bool {
    p.is_file() && p.extension().map(|e| VIDEO_EXTS.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
}

#[derive(Serialize)]
struct CaseCheck {
    label: String,
    ok: bool,
    hint: String,
}

/// 按《工程规范》第 10 节逐条检查一个工程文件夹，只看文件夹这一层能看出来的东西。
/// kind 是 "order" 时按定制工程查，否则按案例工程查
#[tauri::command]
fn check_case(path: String, kind: Option<String>) -> Vec<CaseCheck> {
    let dir = PathBuf::from(path);
    let order = kind.as_deref() == Some("order");
    let files: Vec<String> = fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect()).unwrap_or_default();
    let has = |names: &[&str]| files.iter().any(|f| names.iter().any(|n| f == n));
    let demo = fs::read_dir(dir.join("demo")).map(|rd| rd.flatten().any(|e| is_video(&e.path()))).unwrap_or(false);
    let meta = read_meta(&dir).unwrap_or(Value::Null);
    let filled = |k: &str| !meta_str(&meta, k).is_empty();
    let tagged = meta.get("tags").and_then(|t| t.as_array()).map(|a| !a.is_empty()).unwrap_or(false);
    // 第一层有几个不同名字的 .toe（同一个文件的不同版本算一个）
    let mut bases: Vec<String> = toes_in(&dir).into_iter().map(|t| t.base).collect();
    bases.sort();
    bases.dedup();
    // assets 里（含子文件夹）至少有一个文件
    fn any_file(d: &Path, depth: usize) -> bool {
        fs::read_dir(d).map(|rd| rd.flatten().any(|e| if e.path().is_dir() { depth < 4 && any_file(&e.path(), depth + 1) } else { system_junk(&e.file_name().to_string_lossy()).is_none() })).unwrap_or(false)
    }
    let packed = fs::read_dir(dir.join("deliver")).map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().to_lowercase().ends_with(".zip"))).unwrap_or(false);
    let thumb = has(&["thumb.jpg", "thumb.png", "cover.jpg", "cover.png"]) || meta.get("thumbFile").and_then(|t| t.as_str()).map(|t| t.len() > 1).unwrap_or(false);
    let item = |label: &str, ok: bool, hint: &str| CaseCheck { label: label.into(), ok, hint: hint.into() };
    let toe_hint = if bases.len() > 1 { "第一层有不止一个工程文件，只留主 .toe，别的挪走" } else { "把主 .toe 放在工程文件夹第一层" };
    let mut out = vec![
        item("工程文件在第一层，只有一个", bases.len() == 1, toe_hint),
        item("有说明文档", has(&["readme.md", "readme.txt", "使用说明.md", "使用说明.txt"]), "第一层放一份 README.md：运行环境、依赖、怎么操作、能改哪些参数"),
        item("素材在工程里", any_file(&dir.join("assets"), 0), "用到的图片、视频、模型都放进 assets，路径用相对路径"),
    ];
    if order {
        out.push(item("标注完整", filled("slug") && filled("version") && filled("tdVersion"), "在下面填好英文项目名、工程版本、TD 版本"));
        out.push(item("有预览图", thumb, "用 TD 打开保存一次，或在详情里选一张图"));
        out.push(item("deliver 里留了压缩包", packed, "交付时点「打包交付」，压缩包会留一份在 deliver 文件夹"));
    } else {
        out.push(item("标注完整", tagged && filled("note"), "在详情里填上标签和一句话备注，买家一眼知道这是什么"));
        out.push(item("有预览图", thumb, "用 TD 打开保存一次，或在详情里选一张图"));
        out.push(item("有演示视频", demo, "把成片放进 demo 文件夹（mp4 / mov）"));
    }
    out
}

/* ================= 案例演示视频加水印 ================= */

const WM_DIR: &str = "案例视频";
const WM_SRC: &str = "无水印";
const WM_OUT: &str = "水印版";

#[derive(Serialize)]
struct WmItem {
    rel: String,
    name: String,
    src: String,
    out: String,
    done: bool,
    /// 水印版的体积（字节），没生成是 0
    size: u64,
    /// 水印版旁边有没有封面图
    poster: bool,
}

#[derive(Serialize)]
struct WmScan {
    dir: String,
    exists: bool,
    ffmpeg: Option<String>,
    items: Vec<WmItem>,
}

/// 不弹黑色命令行窗口
fn quiet(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// 找 ffmpeg：只查固定位置（含之前加水印脚本下载到 根目录\_tools 的那份），最后试 PATH
fn find_ffmpeg(root: &Path) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let mut c: Vec<PathBuf> = Vec::new();
    for base in [root.join("_tools"), root.join(WM_DIR).join("_tools")] {
        if let Ok(rd) = fs::read_dir(&base) {
            for e in rd.flatten() {
                c.push(e.path().join("bin").join(exe));
            }
        }
        c.push(base.join(exe));
    }
    for (var, tail) in [("LOCALAPPDATA", "Microsoft/WinGet/Links"), ("ProgramFiles", "ffmpeg/bin"), ("USERPROFILE", "scoop/shims"), ("ProgramData", "chocolatey/bin")] {
        if let Ok(v) = std::env::var(var) {
            c.push(PathBuf::from(v).join(tail).join(exe));
        }
    }
    for d in ["C:/ffmpeg/bin", "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        c.push(PathBuf::from(d).join(exe));
    }
    if let Some(p) = c.into_iter().find(|p| p.is_file()) {
        return Some(p);
    }
    let ok = quiet(std::process::Command::new("ffmpeg").arg("-version")).output().map(|o| o.status.success()).unwrap_or(false);
    ok.then(|| PathBuf::from("ffmpeg"))
}

fn wm_out_path(base: &Path, rel: &str) -> PathBuf {
    base.join(WM_OUT).join(Path::new(rel).with_extension("mp4"))
}

fn wm_collect(src_root: &Path, dir: &Path, depth: usize, base: &Path, out: &mut Vec<WmItem>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with('.') || name.starts_with('_') {
            continue;
        }
        if p.is_dir() {
            if depth < 3 {
                wm_collect(src_root, &p, depth + 1, base, out);
            }
        } else if is_video(&p) {
            let rel = p.strip_prefix(src_root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            let o = wm_out_path(base, &rel);
            // 水印版存在、而且不比原片旧，才算同步好了
            let done = o.is_file() && mtime(&o) >= mtime(&p);
            out.push(WmItem { name: p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), src: p.to_string_lossy().to_string(), size: fs::metadata(&o).map(|m| m.len()).unwrap_or(0), poster: poster_path(&o).is_file(), out: o.to_string_lossy().to_string(), rel, done });
        }
    }
}

/// 读 根目录/案例视频/无水印 里的视频，对照 水印版 看哪些还没同步。create=true 时把这两个文件夹建好
#[tauri::command(async)]
fn wm_scan(root: String, create: bool) -> Result<WmScan, String> {
    let root = PathBuf::from(root);
    if !root.is_dir() {
        return Err("工程根目录不存在".into());
    }
    let base = root.join(WM_DIR);
    if create {
        fs::create_dir_all(base.join(WM_SRC)).map_err(|e| e.to_string())?;
        fs::create_dir_all(base.join(WM_OUT)).map_err(|e| e.to_string())?;
    }
    let src_root = base.join(WM_SRC);
    let mut items = Vec::new();
    wm_collect(&src_root, &src_root, 1, &base, &mut items);
    Ok(WmScan { dir: base.to_string_lossy().to_string(), exists: src_root.is_dir(), ffmpeg: find_ffmpeg(&root).map(|p| p.to_string_lossy().to_string()), items })
}

/// 从 "…, 1920x1080 [SAR…" 这样的文字里找出画面尺寸
fn parse_size(text: &str) -> Option<(u32, u32)> {
    for line in text.lines().filter(|l| l.contains("Video:")) {
        for tok in line.split(|c: char| c == ',' || c == ' ' || c == '[') {
            if let Some((w, h)) = tok.split_once('x') {
                if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                    if w >= 16 && h >= 16 {
                        return Some((w, h));
                    }
                }
            }
        }
    }
    None
}

/// 视频的宽高（用 ffmpeg -i 的输出读，不依赖 ffprobe）
#[tauri::command(async)]
fn wm_probe(root: String, src: String) -> Result<(u32, u32), String> {
    let ff = find_ffmpeg(Path::new(&root)).ok_or("没找到 ffmpeg")?;
    let out = quiet(std::process::Command::new(ff).args(["-hide_banner", "-i"]).arg(&src)).output().map_err(|e| e.to_string())?;
    parse_size(&String::from_utf8_lossy(&out.stderr)).ok_or_else(|| "读不出这个视频的画面尺寸".to_string())
}

#[derive(Deserialize)]
struct WmJob {
    root: String,
    rel: String,
    overlay: Vec<u8>,
    cw: u32,
    ch: u32,
    ix: u32,
    iy: u32,
    iw: u32,
    ih: u32,
}

/// 给一个视频加水印：画面等比缩放放进画布的内框，再把整张水印图（界面按模板画好的 PNG）盖上去。
/// 优先用 NVIDIA 显卡编码，不行再用 CPU。原片不动。
#[tauri::command(async)]
fn wm_render(job: WmJob) -> Result<String, String> {
    let root = PathBuf::from(&job.root);
    if job.rel.split('/').any(|s| s == ".." || s.is_empty()) {
        return Err("视频路径不对".into());
    }
    let base = root.join(WM_DIR);
    let src = base.join(WM_SRC).join(&job.rel);
    if !src.is_file() {
        return Err("原视频找不到了".into());
    }
    let ff = find_ffmpeg(&root).ok_or("没找到 ffmpeg")?;
    let out = wm_out_path(&base, &job.rel);
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let stamp = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let ov = std::env::temp_dir().join(format!("tdl_wm_{}_{}.png", std::process::id(), stamp));
    fs::write(&ov, &job.overlay).map_err(|e| e.to_string())?;
    let tmp = out.with_extension("part.mp4");
    let (cw, ch, ix, iy, iw, ih) = (job.cw, job.ch, job.ix, job.iy, job.iw, job.ih);
    let fc = format!("[0:v]scale={iw}:{ih}:force_original_aspect_ratio=decrease:flags=lanczos,setsar=1,pad={cw}:{ch}:trunc(({ix}+({iw}-iw)/2)/2)*2:trunc(({iy}+({ih}-ih)/2)/2)*2:color=0x0A0A0A[v];[v][1:v]overlay=0:0,format=yuv420p[o]");
    let encoders: [&[&str]; 2] = [&["-c:v", "h264_nvenc", "-preset", "p5", "-rc", "vbr", "-cq", "21", "-b:v", "0"], &["-c:v", "libx264", "-preset", "medium", "-crf", "20"]];
    let mut last = String::new();
    let mut ok = false;
    for enc in encoders {
        let mut cmd = std::process::Command::new(&ff);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(&src).arg("-i").arg(&ov);
        cmd.args(["-filter_complex", &fc, "-map", "[o]", "-map", "0:a?"]).args(enc);
        cmd.args(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]).arg(&tmp);
        match quiet(&mut cmd).output() {
            Ok(o) if o.status.success() => {
                ok = true;
                break;
            }
            Ok(o) => last = String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("").to_string(),
            Err(e) => last = e.to_string(),
        }
    }
    let _ = fs::remove_file(&ov);
    if !ok {
        let _ = fs::remove_file(&tmp);
        return Err(format!("ffmpeg 没能处理这个视频：{}", last));
    }
    let _ = fs::remove_file(&out);
    fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
    // 顺带出封面；封面没出来不算失败，界面上会提示补
    let _ = make_poster(&ff, &out, 3.0);
    Ok(out.to_string_lossy().to_string())
}

/* ---- 官网要的：封面、30 MB 以内 ---- */

const WEB_VIDEO_MAX: u64 = 30 * 1024 * 1024;
const WEB_POSTER_MAX: u64 = 300 * 1024;
const WEB_ZIP_MAX: u64 = 500 * 1024 * 1024;

/// 封面和视频同名，加 _poster，JPG
fn poster_path(video: &Path) -> PathBuf {
    let stem = video.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    video.with_file_name(format!("{}_poster.jpg", stem))
}

/// 从视频第 at 秒取一帧当封面，压到 300 KB 以内：先降画质，还不行再缩小
fn make_poster(ff: &Path, video: &Path, at: f64) -> Result<PathBuf, String> {
    let out = poster_path(video);
    let tmp = out.with_extension("part.jpg");
    let dur = video_duration(ff, video).unwrap_or(0.0);
    let at = if dur > 0.2 && at >= dur { (dur / 2.0).max(0.0) } else { at.max(0.0) };
    for (q, scale) in [(3, 1.0), (6, 1.0), (10, 1.0), (10, 0.75), (14, 0.6), (18, 0.5)] {
        let vf = format!("scale=trunc(iw*{s}/2)*2:trunc(ih*{s}/2)*2", s = scale);
        let mut cmd = std::process::Command::new(ff);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-ss", &format!("{:.2}", at), "-i"]).arg(video);
        cmd.args(["-frames:v", "1", "-vf", &vf, "-q:v", &q.to_string(), "-f", "image2"]).arg(&tmp);
        let ok = quiet(&mut cmd).output().map(|o| o.status.success()).unwrap_or(false);
        if ok && tmp.is_file() && fs::metadata(&tmp).map(|m| m.len() > 0 && m.len() <= WEB_POSTER_MAX).unwrap_or(false) {
            let _ = fs::remove_file(&out);
            fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
            return Ok(out);
        }
    }
    let _ = fs::remove_file(&tmp);
    Err("封面没生成出来".into())
}

/// 从 ffmpeg -i 的输出里读时长（秒）："Duration: 00:01:02.50"
fn parse_duration(text: &str) -> Option<f64> {
    let i = text.find("Duration: ")? + 10;
    let t = text[i..].split(',').next()?.trim();
    let mut parts = t.split(':');
    let (h, m, s) = (parts.next()?.parse::<f64>().ok()?, parts.next()?.parse::<f64>().ok()?, parts.next()?.parse::<f64>().ok()?);
    Some(h * 3600.0 + m * 60.0 + s)
}

fn video_duration(ff: &Path, video: &Path) -> Option<f64> {
    let out = quiet(std::process::Command::new(ff).args(["-hide_banner", "-i"]).arg(video)).output().ok()?;
    parse_duration(&String::from_utf8_lossy(&out.stderr))
}

fn wm_out_checked(root: &str, rel: &str) -> Result<(PathBuf, PathBuf), String> {
    let root = PathBuf::from(root);
    if rel.split('/').any(|s| s == ".." || s.is_empty()) {
        return Err("视频路径不对".into());
    }
    let out = wm_out_path(&root.join(WM_DIR), rel);
    if !out.is_file() {
        return Err("水印版还没生成".into());
    }
    let ff = find_ffmpeg(&root).ok_or("没找到 ffmpeg")?;
    Ok((ff, out))
}

/// 重新取封面：at 是第几秒
#[tauri::command(async)]
fn wm_poster(root: String, rel: String, at: f64) -> Result<String, String> {
    let (ff, out) = wm_out_checked(&root, &rel)?;
    make_poster(&ff, &out, at).map(|p| p.to_string_lossy().to_string())
}

/// 把水印版压到 30 MB 以内：按时长算出码率重新编码，一次不够再降一档
#[tauri::command(async)]
fn wm_shrink(root: String, rel: String) -> Result<u64, String> {
    let (ff, out) = wm_out_checked(&root, &rel)?;
    let dur = video_duration(&ff, &out).filter(|d| *d > 0.5).ok_or("读不出视频时长")?;
    let tmp = out.with_extension("small.mp4");
    for margin in [0.93, 0.85, 0.75] {
        let total = (WEB_VIDEO_MAX as f64 * 8.0 * margin / dur) as i64;
        let v = (total - 128_000).max(200_000);
        let mut cmd = std::process::Command::new(&ff);
        cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(&out);
        cmd.args(["-c:v", "libx264", "-preset", "medium", "-pix_fmt", "yuv420p", "-b:v", &v.to_string(), "-maxrate", &((v as f64 * 1.15) as i64).to_string(), "-bufsize", &(v * 2).to_string()]);
        cmd.args(["-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"]).arg(&tmp);
        let res = quiet(&mut cmd).output().map_err(|e| e.to_string())?;
        if !res.status.success() {
            let _ = fs::remove_file(&tmp);
            return Err(format!("ffmpeg 没能压缩：{}", String::from_utf8_lossy(&res.stderr).lines().last().unwrap_or("")));
        }
        let size = fs::metadata(&tmp).map(|m| m.len()).unwrap_or(u64::MAX);
        if size <= WEB_VIDEO_MAX {
            let _ = fs::remove_file(&out);
            fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
            return Ok(size);
        }
    }
    let _ = fs::remove_file(&tmp);
    Err("压了三次还是超过 30 MB，这个视频太长了，建议剪短".into())
}

/// MP4 能不能边下边播（moov 在 mdat 前面），以及是不是 H.264。只看文件头，不依赖 ffmpeg
fn mp4_web_ok(path: &Path) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(path).map_err(|e| e.to_string())?;
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let mut pos = 0u64;
    while pos + 8 <= len {
        let mut h = [0u8; 8];
        f.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
        f.read_exact(&mut h).map_err(|e| e.to_string())?;
        let mut size = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64;
        let kind = &h[4..8];
        if size == 1 {
            let mut ext = [0u8; 8];
            f.read_exact(&mut ext).map_err(|e| e.to_string())?;
            size = u64::from_be_bytes(ext);
        } else if size == 0 {
            size = len - pos;
        }
        if kind == b"mdat" {
            return Err("没开 faststart（视频信息在画面数据后面），访客要下载完才能播".into());
        }
        if kind == b"moov" {
            let mut buf = vec![0u8; size.min(8 * 1024 * 1024) as usize];
            f.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
            f.read_exact(&mut buf).map_err(|e| e.to_string())?;
            let has = |tag: &[u8]| buf.windows(4).any(|w| w == tag);
            return if has(b"avc1") || has(b"avc3") { Ok(()) } else { Err("不是 H.264 编码".into()) };
        }
        if size < 8 {
            break;
        }
        pos += size;
    }
    Err("不是有效的 MP4".into())
}

/* ================= 打包 ================= */

fn zip_add(zip: &mut zip::ZipWriter<fs::File>, src: &Path, name: &str) -> Result<(), String> {
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
    if src.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(src).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let n = p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
            let l = n.to_lowercase();
            // 交付物里不带：内部标注、备份、崩溃存档、系统杂项
            if l == "project.json" || l == "backup" || l.starts_with("crashautosave") || l == "desktop.ini" || l == ICON_FILE || system_junk(&n).is_some() {
                continue;
            }
            zip_add(zip, &p, &format!("{}/{}", name, n))?;
        }
        Ok(())
    } else {
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        let mut f = fs::File::open(src).map_err(|e| e.to_string())?;
        std::io::copy(&mut f, zip).map(|_| ()).map_err(|e| e.to_string())
    }
}

/// 把工程打成交付用的压缩包，写到 out。只带：主 .toe、README、assets、tox、scripts；
/// 案例工程另带 demo 和 thumb.jpg。project.json、docs、deliver、Backup、export、多余的 .toe 都不带。
fn pack_to(dir: &Path, out: &Path, case: bool) -> Result<u64, String> {
    let meta = read_meta(dir);
    let prefer = meta.as_ref().and_then(|m| m.get("mainToe")).and_then(|v| v.as_str()).map(|s| s.to_string());
    let toe = main_toe(&toes_in(dir), prefer.as_deref()).ok_or("工程文件夹第一层没有 .toe，没法打包")?;
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = out.with_extension("part");
    let res = (|| -> Result<(), String> {
        let mut zip = zip::ZipWriter::new(fs::File::create(&tmp).map_err(|e| e.to_string())?);
        let top = out.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        zip_add(&mut zip, &toe, &format!("{}/{}", top, toe.file_name().unwrap().to_string_lossy()))?;
        let mut keep: Vec<&str> = vec!["assets", "tox", "scripts"];
        if case {
            keep.push("demo");
        }
        for e in fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            let l = n.to_lowercase();
            let take = if e.path().is_dir() { keep.contains(&l.as_str()) } else { l == "readme.md" || l == "readme.txt" || l.starts_with("使用说明.") || (case && l == "thumb.jpg") };
            if take {
                zip_add(&mut zip, &e.path(), &format!("{}/{}", top, n))?;
            }
        }
        zip.finish().map(|_| ()).map_err(|e| e.to_string())
    })();
    if let Err(e) = res {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    let _ = fs::remove_file(out);
    fs::rename(&tmp, out).map_err(|e| e.to_string())?;
    Ok(fs::metadata(out).map(|m| m.len()).unwrap_or(0))
}

fn meta_str(meta: &Value, key: &str) -> String {
    meta.get(key).and_then(|v| v.as_str()).unwrap_or("").trim().to_string()
}

#[derive(Serialize)]
struct Packed {
    path: String,
    size: u64,
}

/// 定制工程打包：存到工程的 deliver 文件夹，名字是 英文项目名_v版本_日期.zip
#[tauri::command(async)]
fn pack_order(path: String, date: String) -> Result<Packed, String> {
    let dir = PathBuf::from(&path);
    let meta = read_meta(&dir).ok_or("这个工程还没有标注")?;
    let (slug, ver) = (meta_str(&meta, "slug"), meta_str(&meta, "version"));
    if slug.is_empty() || ver.is_empty() {
        return Err("先在详情里填好英文短名和工程版本".into());
    }
    let out = dir.join("deliver").join(format!("{}_v{}_{}.zip", slug, ver, date.replace('-', "")));
    let size = pack_to(&dir, &out, false)?;
    Ok(Packed { path: out.to_string_lossy().to_string(), size })
}

const WEB_DIR: &str = "_官网上传";

#[derive(Serialize)]
struct WebPack {
    dir: String,
    files: Vec<String>,
    notes: Vec<String>,
}

fn collect_mp4(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth < 3 {
                collect_mp4(&p, depth + 1, out);
            }
        } else if p.extension().map(|x| x.to_string_lossy().to_lowercase() == "mp4").unwrap_or(false) {
            out.push(p);
        }
    }
}

/// 视频文件名是不是「编号_短名_比例[_v版本]」
fn web_video_name(stem: &str, case_id: &str, slug: &str) -> bool {
    let Some(rest) = stem.strip_prefix(&format!("{}_{}_", case_id, slug)) else { return false };
    let mut parts = rest.split('_');
    let ratio_ok = matches!(parts.next(), Some("16x9" | "9x16" | "3x4" | "4x3"));
    let ver_ok = match parts.next() {
        None => true,
        Some(v) => v.len() > 1 && v.starts_with('v') && v[1..].chars().all(|c| c.is_ascii_digit()),
    };
    ratio_ok && ver_ok && parts.next().is_none()
}

/// 准备官网上传包：根目录/_官网上传/案例标识/ 里正好放官网要的东西。
/// 视频只从 案例视频/水印版 里取，绝不碰无水印原片。有不合格的就列出来，什么都不生成。
#[tauri::command(async)]
fn web_pack(root: String, path: String) -> Result<WebPack, String> {
    let root = PathBuf::from(root);
    let dir = PathBuf::from(&path);
    let meta = read_meta(&dir).ok_or("这个工程还没有标注")?;
    let (case_id, slug, ver) = (meta_str(&meta, "caseId"), meta_str(&meta, "slug"), meta_str(&meta, "version"));
    if case_id.is_empty() || slug.is_empty() || ver.is_empty() {
        return Err("编号、英文短名、工程版本要先填好".into());
    }
    let mut vids = Vec::new();
    collect_mp4(&root.join(WM_DIR).join(WM_OUT), 1, &mut vids);
    vids.retain(|p| p.file_stem().map(|s| web_video_name(&s.to_string_lossy(), &case_id, &slug)).unwrap_or(false));
    vids.sort();
    let mut bad = Vec::new();
    if vids.is_empty() {
        bad.push(format!("「案例视频/水印版」里没有这个案例的视频（文件名要是 {}_{}_16x9.mp4 这样）", case_id, slug));
    }
    for v in &vids {
        let name = v.file_name().unwrap().to_string_lossy().to_string();
        let size = fs::metadata(v).map(|m| m.len()).unwrap_or(0);
        if size > WEB_VIDEO_MAX {
            bad.push(format!("{} 有 {:.1} MB，超过 30 MB", name, size as f64 / 1048576.0));
        }
        if let Err(e) = mp4_web_ok(v) {
            bad.push(format!("{}：{}", name, e));
        }
        let poster = poster_path(v);
        match fs::metadata(&poster) {
            Ok(m) if m.len() <= WEB_POSTER_MAX => {}
            Ok(_) => bad.push(format!("{} 的封面超过 300 KB", name)),
            Err(_) => bad.push(format!("{} 还没有封面", name)),
        }
    }
    if toes_in(&dir).is_empty() {
        bad.push("工程文件夹第一层没有 .toe".into());
    }
    if !bad.is_empty() {
        return Err(bad.join("\n"));
    }
    let dest = root.join(WEB_DIR).join(format!("{}-{}", case_id, slug));
    // 重新生成：先清掉上次放进去的文件（只清这一层的文件，不动别的）
    if let Ok(rd) = fs::read_dir(&dest) {
        for e in rd.flatten().filter(|e| e.path().is_file()) {
            let _ = fs::remove_file(e.path());
        }
    }
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    let mut notes = Vec::new();
    fs::copy(dir.join("project.json"), dest.join("project.json")).map_err(|e| e.to_string())?;
    files.push("project.json".to_string());
    for v in &vids {
        for f in [v.clone(), poster_path(v)] {
            let n = f.file_name().unwrap().to_string_lossy().to_string();
            fs::copy(&f, dest.join(&n)).map_err(|e| e.to_string())?;
            files.push(n);
        }
    }
    let zip_name = format!("{}-{}_v{}.zip", case_id, slug, ver);
    let zip_path = dest.join(&zip_name);
    let size = pack_to(&dir, &zip_path, true)?;
    if size > WEB_ZIP_MAX {
        // 太大的不放进上传包，挪到旁边，留给网盘
        let aside = root.join(WEB_DIR).join(&zip_name);
        let _ = fs::remove_file(&aside);
        fs::rename(&zip_path, &aside).map_err(|e| e.to_string())?;
        notes.push(format!("压缩包有 {:.0} MB，超过 500 MB，官网收不了。已放在 {} 旁边，请传网盘，把链接和提取码填进官网后台", size as f64 / 1048576.0, WEB_DIR));
    } else {
        notes.push(format!("压缩包 {:.1} MB", size as f64 / 1048576.0));
        files.push(zip_name);
    }
    Ok(WebPack { dir: dest.to_string_lossy().to_string(), files, notes })
}

/// 界面用来判断组件库里的路径还在不在："file" / "dir" / ""
#[tauri::command]
fn path_kind(path: String) -> String {
    let p = Path::new(&path);
    if p.is_file() { "file".into() } else if p.is_dir() { "dir".into() } else { String::new() }
}


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 不能多开（单实例）：已经开着时再点图标，不开第二个窗口，而是把已有的窗口叫到前面。这个插件要第一个注册
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![scan_projects, inspect_import, set_thumb, list_templates, open_templates_dir, save_as_template, create_project, save_meta, open_project, reveal, delete_project, move_project, set_folder_icon, read_image, save_custom_logo, clear_custom_logo, custom_logo, list_user_themes, open_themes_dir, check_case, wm_scan, wm_probe, wm_render, wm_poster, wm_shrink, pack_order, web_pack, path_kind])
        .run(tauri::generate_context!())
        .expect("启动失败");
}
