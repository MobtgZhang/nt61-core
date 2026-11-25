# NT61 串口调试工具

在仓库根目录编译，生成宿主机可执行文件 `target/debug/nt61-debug`。

```sh
cargo build --locked -p nt61-tools --bin nt61-debug
target/debug/nt61-debug /path/to/serial.log \
  --project-image /path/to/project-image.json \
  --require-desktop --deny-panic \
  --require-exit 'C:\muiprobe.exe' --require-jit 'C:\muiprobe.exe'
```

报告列出项目启动链、桌面就绪、PE 加载失败、客体执行异常、翻译计数、退出码和内核 panic，行号对应原始串口日志。完整的调度切换记录可从被打断的串口行中剥离。工具忽略未换行的残缺尾部，并保留路径损坏的故障记录供排查。

使用 `--json` 输出结构化报告，用 `-` 从标准输入读取。`--require-exit` 检查指定程序最近一次可识别的启动是否退出 `0`。`--require-jit` 检查该次运行是否记录退出，以及原生执行和解释执行的计数是否均非零。两个参数都接受完整 Windows 路径，且可重复指定。`--project-image` 接受验收脚本生成的清单，报告清单摘要字段之间以及启动标记的一致性。它读取清单中的摘要，适用于磁盘镜像已经不在本机的日志归档。

检查客体 PE 的直接导入及其导出转发链时，给出项目构建目录中对应位宽的 `System32` 或 `SysWOW64`。报告覆盖缺失 DLL、位宽不符、缺失的名称或序号导出以及转发循环。报告同时记录失败的原始导入和经过的转发目标。DLL 的间接静态依赖、初始化和实际执行仍需由客体链接器和串口日志验证。

```sh
target/debug/nt61-debug --pe-imports /path/to/notepad.exe \
  --runtime-dir build/x86_64/ntfs/system/Windows/System32 --json
```

退出码 `0` 表示报告已输出且所选检查通过，`1` 表示检查失败，`2` 表示输入或命令参数错误。默认只报告观察结果。回归测试可按需组合 `--require-boot`、`--require-desktop`、`--deny-panic`、`--require-exit` 和 `--require-jit`。
