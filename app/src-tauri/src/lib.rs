//! TD 工程启动器 · 后端
//! 只做"工程文件夹"这一层的事：扫描识别、新建、导入、读写 project.json、打开、在文件夹中显示、删除（回收站）、移动、文件夹图标。

use serde::Serialize;
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

/// 去掉 "00_"、"99-" 这类数字前缀
fn strip_num_prefix(name: &str) -> &str {
    let t = name.trim_start_matches(|c: char| c.is_ascii_digit());
    if t.len() < name.len() {
        t.trim_start_matches(['_', '-', ' ', '.'])
    } else {
        name
    }
}

/// 扫描时整个跳过的文件夹；top 表示它直接在根目录下
fn skip_dir(name: &str, top: bool) -> bool {
    let l = name.to_lowercase();
    l.starts_with('.')
        || l.starts_with('_')
        || l == "backup"
        || l == "node_modules"
        || system_junk(name).is_some()
        || (top && strip_num_prefix(&l) == "library")
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

/// 分类文件夹最多往下找几层（例如 根目录/01_商单/客户A/工程）
const MAX_DEPTH: usize = 4;
/// 工程里面再往下找几层带 project.json 的子工程
const NESTED_DEPTH: usize = 3;

#[tauri::command]
fn scan_projects(root: String, extra: Option<Vec<String>>) -> Result<Vec<Project>, String> {
    let root = PathBuf::from(&root);
    fs::read_dir(&root).map_err(|e| format!("{}（{}）", e, root.display()))?;
    let mut out = Vec::new();
    scan_dir(&root, "", 1, &mut out);
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
    let mut p = read_project(dir, &folder_name(dir), EXT_GROUP)?;
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
    if let Some(p) = read_project(&dir, &folder_name(&dir), EXT_GROUP) {
        out.push(p);
    } else {
        scan_dir(&dir, "", 1, &mut out);
        // 选的是根目录的上级时，根目录里的工程本来就在，不算导入
        out.retain(|p| !inside(&root, Path::new(&p.path)));
    }
    for p in &mut out {
        p.group = EXT_GROUP.to_string();
        p.external = true;
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(ImportScan { inside_root: false, projects: out })
}

fn join_group(group: &str, folder: &str) -> String {
    if group.is_empty() {
        folder.to_string()
    } else {
        format!("{}/{}", group, folder)
    }
}

/// 是工程就收下，并只在它里面找带 project.json 的子工程；不是工程就当分类文件夹往下找
fn scan_dir(dir: &Path, group: &str, depth: usize, out: &mut Vec<Project>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let sub = e.path();
        if !sub.is_dir() {
            continue;
        }
        let folder = e.file_name().to_string_lossy().to_string();
        if skip_dir(&folder, depth == 1) {
            continue;
        }
        if let Some(p) = read_project(&sub, &folder, group) {
            out.push(p);
            scan_nested(&sub, &join_group(group, &folder), 1, out);
        } else if depth < MAX_DEPTH {
            scan_dir(&sub, &join_group(group, &folder), depth + 1, out);
        }
    }
}

/// 工程内部：附带的 .toe（组件、参考源文件）不算工程，只认有 project.json 的子文件夹
fn scan_nested(dir: &Path, group: &str, depth: usize, out: &mut Vec<Project>) {
    if depth > NESTED_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let sub = e.path();
        if !sub.is_dir() {
            continue;
        }
        let folder = e.file_name().to_string_lossy().to_string();
        if skip_dir(&folder, false) {
            continue;
        }
        if sub.join("project.json").is_file() {
            if let Some(p) = read_project(&sub, &folder, group) {
                out.push(p);
                scan_nested(&sub, &join_group(group, &folder), 1, out);
                continue;
            }
        }
        scan_nested(&sub, &join_group(group, &folder), depth + 1, out);
    }
}

fn read_project(dir: &Path, folder: &str, group: &str) -> Option<Project> {
    let has_meta = dir.join("project.json").is_file();
    let toes = toes_in(dir);
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
        thumb: has_thumb.then(|| thumb.to_string_lossy().to_string()),
        thumb_mtime: if has_thumb { mtime(&thumb) } else { 0 },
        modified,
        has_meta,
        meta: meta.unwrap_or(Value::Null),
        external: false,
        folder_icon: has_folder_icon(dir),
    })
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

#[tauri::command]
fn create_project(app: AppHandle, root: String, folder: String, name: String, created: String) -> Result<Created, String> {
    create_project_at(root, folder, name, created, template_toe(&app))
}

fn create_project_at(root: String, folder: String, name: String, created: String, tpl: Option<PathBuf>) -> Result<Created, String> {
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
    for sub in SUBDIRS {
        fs::create_dir_all(dir.join(sub)).map_err(|e| e.to_string())?;
    }
    let meta = json!({
        "name": name,
        "tags": [],
        "status": "进行中",
        "client": "",
        "note": "",
        "created": created,
    });
    fs::write(dir.join("project.json"), serde_json::to_string_pretty(&meta).unwrap()).map_err(|e| e.to_string())?;
    let mut toe = None;
    if let Some(tpl) = tpl {
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
    let toe = main_toe(&toes_in(dir), prefer.as_deref()).ok_or("这个工程里没有 .toe 文件")?;
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
    if toes_in(&dir).is_empty() && !dir.join("project.json").is_file() {
        return Err("这个文件夹不像工程（没有 .toe 也没有 project.json），没有动它".into());
    }
    Ok(dir)
}

/// 检查 path 是一个工程文件夹（不要求在根目录里），给导入的工程用
fn check_project(path: &Path) -> Result<PathBuf, String> {
    let dir = path.canonicalize().map_err(|_| "工程文件夹不存在".to_string())?;
    if !dir.is_dir() || (toes_in(&dir).is_empty() && !dir.join("project.json").is_file()) {
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

        let c = create_project_at(r.clone(), "2026-11-17_粒子雨".into(), "粒子雨".into(), "2026-11-17".into(), Some(tpl.clone())).unwrap();
        let dir = PathBuf::from(&c.path);
        for sub in SUBDIRS {
            assert!(dir.join(sub).is_dir(), "缺少 {}", sub);
        }
        assert!(dir.join("粒子雨.toe").is_file());
        assert_eq!(c.toe.is_some(), true);

        // 同名再建一次应当报错
        assert!(create_project_at(r.clone(), "2026-11-17_粒子雨".into(), "粒子雨".into(), "x".into(), None).is_err());
        // 非法字符
        assert!(create_project_at(r.clone(), "a:b".into(), "a:b".into(), "x".into(), None).is_err());

        // 旧工程：只有 .toe（带版本号）和 thumb.jpg，没有 project.json
        let old = root.join("旧工程");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("demo.12.toe"), b"x").unwrap();
        fs::write(old.join("thumb.jpg"), b"x").unwrap();
        // 不是工程的文件夹 + Backup 文件夹应被跳过
        fs::create_dir_all(root.join("随便的文件夹")).unwrap();
        fs::create_dir_all(root.join("Backup")).unwrap();
        fs::write(root.join("Backup").join("x.toe"), b"x").unwrap();

        // 分类文件夹里的工程：01_商单/客户A/海报 应被找到，group = 01_商单/客户A
        let nested = root.join("01_商单").join("客户A").join("海报");
        fs::create_dir_all(nested.join("Backup")).unwrap();
        fs::write(nested.join("poster.toe"), b"x").unwrap();
        fs::write(nested.join("Backup").join("poster.3.toe"), b"x").unwrap();

        let list = scan_projects(r.clone(), None).unwrap();
        assert_eq!(list.len(), 3, "应只扫到 3 个工程");
        let n = list.iter().find(|p| p.folder == "海报").unwrap();
        assert_eq!(n.group, "01_商单/客户A");
        assert_eq!(n.name, "海报");
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
        // 工程里有 project.json 的子文件夹单独算
        mk("02_案例/Skin/2026-11-17_Sub", &["project.json", "sub.toe"]);
        // 零散 .toe → maybe
        mk("loose", &["a.toe"]);
        // 只剩 Backup → 不算
        mk("only_backup/Backup", &["x.3.toe"]);
        // Mac 影子文件 / __MACOSX / _开头 / 第一层库文件夹 → 都不算
        mk("mac_zip", &["._ghost.toe"]);
        mk("mac_zip/__MACOSX/inner", &["._a.toe"]);
        mk("_to_delete/old", &["old.toe"]);
        mk("00_Library/Mediapipe", &["m.toe"]);
        // 不在第一层的 Library 照样扫
        mk("03_x/Library", &["lib.toe"]);

        let list = scan_projects(root.to_string_lossy().to_string(), None).unwrap();
        let names: Vec<_> = list.iter().map(|p| format!("{}|{}", p.group, p.folder)).collect();
        assert_eq!(list.len(), 4, "{:?}", names);
        let skin = list.iter().find(|p| p.folder == "Skin").unwrap();
        assert_eq!(skin.confidence, "likely");
        assert_eq!(skin.toe_bases, vec!["Other".to_string(), "Skin".to_string()]);
        assert!(!skin.toe.as_ref().unwrap().contains("CrashAutoSave"));
        assert!(skin.earliest > 0);
        let sub = list.iter().find(|p| p.folder == "2026-11-17_Sub").unwrap();
        assert_eq!(sub.group, "02_案例/Skin");
        assert_eq!(sub.confidence, "meta");
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
        assert_eq!(one.projects[0].group, EXT_GROUP);
        assert_eq!(one.projects[0].path, s(&single));
        // 拖进来的是 .toe 文件：看它所在的文件夹
        assert_eq!(inspect_import(r.clone(), s(&single.join("s.toe"))).unwrap().projects.len(), 1);
        // 选的是上级文件夹：列出里面的工程，跳过 Backup 和不是工程的
        let many = inspect_import(r.clone(), s(&outside.join("一批"))).unwrap();
        let mut names: Vec<_> = many.projects.iter().map(|p| p.folder.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["乙".to_string(), "甲".to_string()]);
        // 已经在根目录里的不用导入；选根目录的上级时不把根目录里的工程算进去
        assert!(inspect_import(r.clone(), s(&root.join("inroot"))).unwrap().inside_root);
        let up = inspect_import(r.clone(), s(&base)).unwrap();
        assert!(up.projects.iter().all(|p| p.folder != "inroot"), "根目录里的工程不该出现");
        assert_eq!(up.projects.len(), 3);
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
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![scan_projects, inspect_import, create_project, save_meta, open_project, reveal, delete_project, move_project, set_folder_icon, read_image, save_custom_logo, clear_custom_logo, custom_logo])
        .run(tauri::generate_context!())
        .expect("启动失败");
}
