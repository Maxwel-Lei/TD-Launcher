template.toe 是新建工程用的模板。新建工程时，软件会把它复制到新工程文件夹，并改名为「工程名.toe」。
里面放了 TDL_Loader（组件载入器）：工程第一次打开时，按 project.json 里的 components 把新建时勾选的组件载进 /project1，
并设成外部引用（External .tox 指向工程 tox/ 里的文件），载完记成 loaded，下次不再重复载。
缩略图截取是 ../components/TDL_Thumbnail.tox，作为默认勾选的组件随新工程一起复制、由载入器载入。
