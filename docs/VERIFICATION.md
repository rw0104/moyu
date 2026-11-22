# 0.6.0 发布验证

文中 `artifacts/` 路径指本地构建或验收产物。从源码生成安装包的步骤见[开发手册](DEVELOPMENT.md#14-发布与升级流程)。

日期：2026-10-23。平台：Windows x64。

本版新增独立双向对话模式、自动客户语言跟随与安全文字排版，保留标准连续翻译、完整历史、图片上传/粘贴、截图快捷键和托盘。开发规则见 [完整手册](DEVELOPMENT.md)，演进见 [版本记录](CHANGELOG.md)。

| 检查 | 实际结果与范围 |
| --- | --- |
| TypeScript 与前端生产构建 | npm run build 通过，最终生产资源随 Tauri 构建内嵌 |
| Rust 默认测试 | 51 项通过，0 失败；4 项 opt-in 测试默认忽略并已单独执行通过，见后端报告（`artifacts/backend-tests.json`） |
| 界面回归 | 完整 51 项通过，保留原 39 项并增加 12 项双向对话；另补一项模式附件隔离检查。无页面错误或外部请求，见界面报告（`artifacts/ui-smoke.json`） |
| 对话专项 | 12 项真实组件 + 官方 mockIPC 检查通过，见 专项报告（`artifacts/dialogue-ui-smoke.json`） |
| 紧凑布局 | 3 项通过，覆盖 1280×850、1920×1080、900×640，见 布局报告（`artifacts/compact-layout.json`） |
| 真实语言识别与双向翻译 | 五条合成文本分类、四条中英/中德来往信息的分类+翻译+审核通过，编号保留，见 对话 API 报告（`artifacts/dialogue-api-smoke.json`） |
| 真实图片链路 | 合成英文 PNG 识别、图像核对、中文翻译和语义检查通过，见 图片 API 报告（`artifacts/model-api-smoke.json`） |
| 原生截图与热键 | 真实 RegisterHotKey 冲突/释放/录入恢复通过；合成 DIB 与隐藏 HWND 选区/取消测试通过，见 原生报告（`artifacts/capture-smoke.json`） |
| Windows Release / NSIS | Release / NSIS 构建通过，安装包 4,449,276 字节，内嵌 JS/CSS 与当前构建一致；详见 发布核对记录（`artifacts/build-info.json`） |

安装包：Moyu-0.6.0-windows-x64-setup.exe（`artifacts/Moyu-0.6.0-windows-x64-setup.exe`）。打包脚本检查同版本七份验证报告、当前 JS/CSS 内嵌和测试夹具隔离；八份 JSON（含 build-info）另存于 版本报告目录（`artifacts/releases/0.6.0/`）。

安装包 SHA-256：`4a8c7e2acd952aea58487a1e8e9b2127fc919ba9c1eaf410b88c7ad3d1c09c02`。生产资源 `index-BduG9RYR.js` 与 `index-Cis-1Isa.css` 已核对内嵌，未包含测试夹具。

## 双向对话覆盖

界面测试覆盖中文自动译英、英文自动译中；德语来信后中文回复跟随德语；固定法语后收到德语仍译中但不修改固定回复语言；中性短句方向提示与当前条人工纠偏；识别失败保留输入；翻译失败不重复检测；图片和截图 OCR 后进入同一路由；截图语言识别失败复用已识别文字；两个模式分别恢复会话、草稿和附件；打开历史恢复模式；段落/列表/代码与 HTML 字符安全显示。

后端测试补充自动路由、中文简繁、日文纯汉字不被字符计数强判、中文夹 SKU 仍完整进入分类、长文首中尾 Unicode 采样、非法/重复/多余字段与数组拒绝；工作区和客户语言跨重启恢复；空会话切换；关闭历史保存时模式切回恢复当前进程内容；全局重置不复活非活动参考；反向上下文对齐且不混入其他客户、第三语言或未校验内容。模型对象字段解析现在显式要求顶层 object，拒绝 positional array。

截图：双向来往布局（`artifacts/dialogue-bilingual-thread.png`）、安全文字排版（`artifacts/dialogue-safe-readable-text.png`）。这些截图来自明确标记的 mock 测试页面，译文是测试夹具，不能当作实际翻译质量证据。实际模型结果见下一节。

## 真实模型证据

命令：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib configured_account_ -- --ignored --nocapture --test-threads=1
```

三项真实 API 测试全部通过，模型为 deepseek-flash，固定官方 HTTPS 地址。测试读取已有系统凭据但不打印密钥；仅发送合成文本和合成图片，不读取用户历史，不捕获或上传用户桌面。

| 流程 | 合成输入 | 实际译文 |
| --- | --- | --- |
| 英文 → 中文 | Could you confirm whether item AX-204 is in stock? | 你能确认一下 AX-204 这个商品是否有库存吗？ |
| 中文 → 英文 | AX-204 目前有现货，明天发货。 | AX-204 is currently in stock and will ship tomorrow. |
| 德语 → 中文 | Ist die Bestellung 4821 bereits versandt? | 订单4821是否已经发货？ |
| 跟随德语，中文 → 德语 | 订单 4821 已发货，追踪号为 DE778899。 | Die Bestellung 4821 wurde bereits versandt, die Sendungsnummer lautet DE778899. |

另外五条分类样本覆盖英文、中文夹商品编号、德语、随后中文跟随德语、日语；图片样本识别“Hello, world.”与“Translate only this text.”并译为“你好，世界。”与“只翻译这段文字。”。

本次对话分类测试 5 次请求，四条双向翻译共 12 次请求，图片链路 4 次请求，合计 21 次。它们证明这些具体路径已实际运行，不能推导全部语种、短句、混合语、复杂图片和提示词注入都可靠。语言分类与同模型语义审核仍可能误判；当前条消息可人工纠正方向。完整边界见 [安全说明](SECURITY.md)。

## 原生验证范围

原生热键测试在隔离线程临时注册少见组合，验证替换占用时保留旧键、停用后释放、录入暂停与恢复、timer 清理，不注入真实按键。隐藏的测试专用 HWND 使用生产 WndProc 和合成像素，验证 Esc、右键、显示器变化、取消模式、失焦、失去捕获与松开选区，并核对 PNG 尺寸和像素。默认测试也覆盖反向拖动和负坐标几何。

本轮没有驱动用户桌面上的完整热键→框选→前台恢复链路。物理多屏、混合缩放、远程桌面、受保护窗口没有全面现场验收，GDI 可能不能读取部分安全或受保护内容。原生注册/选区、前端事件/队列和真实 OCR 分层验证，不把 mock 结果冒充原生现场验证。

托盘原生隐藏的既有专项记录为 0.3.0 的专用实例，见 原生托盘报告（`artifacts/native-tray-smoke.json`）；0.6.0 保留实现，未对用户已安装应用进行安装覆盖或托盘操作。

## 重现与发布

```powershell
npm ci
npm run build
npm test
# 终端一：
npm run dev
# 终端二：
python tests/ui/smoke.py
python tests/ui/compact.py
# 结束任务自己的开发服务后：
cargo test --manifest-path src-tauri/Cargo.toml --lib native_hotkey_conflict_preserves_old_key_and_disable_releases_it -- --ignored --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --lib configured_account_ -- --ignored --nocapture --test-threads=1
npm run tauri build
python scripts/package-artifact.py
```

界面测试需要 Python Playwright 与 Chromium；真实模型测试需要有效密钥、网络和余额。生产脚本的版本、七份报告状态、安装包复制和内嵌资源核对必须一致。旧版本报告保存在版本目录：0.5.0（`artifacts/releases/0.5.0/`）。早期安装包见 CHANGELOG，不能把旧报告改版本号当作复测。
