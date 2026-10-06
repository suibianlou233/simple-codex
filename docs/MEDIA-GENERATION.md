# Media generation / 图片与视频生成

Simple 的媒体生成功能是独立于聊天 `ModelAdapter` 的本地优先能力。当前首个供应商实现使用火山方舟 Seedream 与 Seedance，但媒体任务、资产仓库和对话渲染不依赖聊天模型配置。

## 当前能力

- Seedream 文生图、单参考图生图，支持 1K、1.5K、2K。
- Seedance 文生视频和首帧生视频，支持用户附件 `simple-image:` 或当前对话中已生成的 `simple-media:` 图片作为首帧，并支持任务创建、状态轮询和排队取消。
- 生成结果立即下载到本地项目隔离的内容寻址媒体目录。
- 内置 `generate-media` 技能按用户在当前消息中的明确生成意图调用 `simple_media` 本地工具。
- 图片和视频通过 `simple-media:<sha256>` 不透明引用直接显示在普通对话中。
- 视频任务记录保存在本地；超时或中断后可以按本地任务 ID 继续查询，不会重复提交。

## 配置与数据边界

在“设置 → 图片与视频设置”中填写火山方舟 API Key。Key 只保存到操作系统凭据库，媒体状态文件、项目、技能、对话、日志和测试中都不保存密钥。也可以在启动 Simple 前设置 `ARK_API_KEY` 环境变量。

默认接口为 `https://ark.cn-beijing.volces.com/api/v3`。非本机地址只接受 HTTPS；本机回环地址可使用 HTTP，便于契约测试和后续接入本地媒体服务。

媒体状态保存在应用本地数据目录的 `media-generation/state.json`，生成文件保存在 `media-assets/<project-scope>/`。远程临时 URL 不作为持久资产来源。

“这张图”、“这只小狗”等指代只在当前对话中有唯一明确的已生成图片时自动解析。工具仍会在后端按当前项目、64 位内容哈希和真实图片格式重新校验，不接受跨项目引用或视频资产伪装的首帧。

## 付费与失败语义

图片或视频请求只有在用户当前消息明确要求生成时，内置技能才允许调用。一次明确请求只提交一次；若提交期间发生超时，Simple 将任务标记为“结果待确认”，不会自动重试，以免重复计费。

Seedance 任务由本地工具轮询。对话被取消时，已经提交的远程任务仍保留在本地记录中，后续只能查询，不能假装远程任务已经取消。

按照当前方舟接口限制，Seedance 2.5 使用首帧时必须选择自适应画幅；Seedance 2.0/2.5 不支持直接上传含真人人脸的参考图片或视频。技能在未明确指定画幅时会为首帧任务使用自适应画幅。

## 验证

常规单元测试和构建不会调用真实模型。真实验证应使用用户自己的凭据，在低规格下分别完成一次图片和视频生成，并检查本地下载、应用重启恢复和账单记录。

官方接口参考：[图片生成 API](https://docs.volcengine.com/docs/ark/image-generation-api?lang=zh&redirect=1)、[视频任务创建 API](https://docs.volcengine.com/docs/ark/create-video-generation-task-api?lang=zh)、[视频任务查询 API](https://docs.volcengine.com/docs/ark/get-video-generation-task-api?lang=zh)。
