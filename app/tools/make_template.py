# TD 工程启动器 · 生成模板 template.toe
#
# 用法（只需做一次）：
#   1. 打开 TouchDesigner，File > New 新建一个空工程
#   2. Alt+T 打开 Textport，粘贴下面这一行回车：
#        exec(open(r'F:\Projects\VibeCoding\TDFileManager\app\tools\make_template.py', encoding='utf-8').read())
#   3. 看到 "模板已保存" 就完成了。以后在启动器里新建的工程都会带上自动存缩略图的功能。
#
# 它做了什么（企划 §5）：
#   - 在 /project1 里放一个 Resolution TOP（宽 640）+ 名为 thumb 的 Null TOP
#   - 放一个 Execute DAT，打开 Project Post Save：每次 Ctrl+S 都把 thumb 存成工程文件夹里的 thumb.jpg
#   - 把这个工程另存为 app/src-tauri/resources/template/template.toe

import os

OUT = r'F:\Projects\VibeCoding\TDFileManager\app\src-tauri\resources\template\template.toe'
P = op('/project1')

# ---- 缩略图链：最终画面 -> thumb_res -> thumb ----
for n in ('thumb', 'thumb_res', 'thumb_saver'):
    if P.op(n):
        P.op(n).destroy()

res = P.create(resolutionTOP, 'thumb_res')
null = P.create(nullTOP, 'thumb')
res.nodeX, res.nodeY = 600, -300
null.nodeX, null.nodeY = 775, -300
null.inputConnectors[0].connect(res)
try:
    res.par.outputresolution = 'custom'
    res.par.resolutionw = 640
    res.par.resolutionh = 360
except Exception as e:
    print('分辨率参数没设上，请手动把 thumb_res 设为 640x360：', e)

# 自动接到工程里最可能是"最终画面"的 TOP：优先 out1 / final / output，否则找最右边的 TOP
src = None
for name in ('out1', 'final', 'output', 'null_output', 'null1'):
    o = P.op(name)
    if o is not None and o.family == 'TOP' and o is not null:
        src = o
        break
if src is None:
    tops = [o for o in P.children if o.family == 'TOP' and o.name not in ('thumb', 'thumb_res')]
    if tops:
        src = max(tops, key=lambda o: o.nodeX)
if src is not None:
    res.inputConnectors[0].connect(src)
    print('thumb_res 已接到', src.path, '——以后把你的最终画面接到 thumb_res 即可')
else:
    print('没找到可接的 TOP：以后把最终画面接到 thumb_res 的输入')

# ---- 保存时存缩略图 ----
saver = P.create(executeDAT, 'thumb_saver')
saver.nodeX, saver.nodeY = 775, -450
saver.text = '''# 每次保存工程后，把 thumb 存成工程文件夹里的 thumb.jpg（TD 工程启动器用来显示预览）
def onProjectPostSave():
\tsrc = op('/project1/thumb')
\tif src is not None and src.inputs and not src.errors():
\t\ttry:
\t\t\tsrc.save(project.folder + '/thumb.jpg')
\t\texcept Exception as e:
\t\t\tprint('thumb.jpg 保存失败', e)
\treturn
'''
# 打开 Project Post Save 开关（不同版本参数名可能略有差别，按名字里带 postsave 的找）
on = False
for p in saver.pars():
    if 'postsave' in p.name.lower():
        p.val = True
        on = True
for p in saver.pars():
    if p.name.lower() == 'active':
        p.val = True
if not on:
    print('没找到 Project Post Save 开关，请在 thumb_saver 的参数里手动打开它')

os.makedirs(os.path.dirname(OUT), exist_ok=True)
project.save(OUT)
print('模板已保存：', OUT)
