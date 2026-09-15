# 模型接入与等待时间

## 千问 Token Plan

模型设置选择「千问 Token Plan」，自动填写：

- 接口：https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1
- 模型：qwen3.8-max
- 协议：千问 OpenAI Chat Completions 兼容接口
- 上下文：1,000,000 Token；默认输出预算：32,768 Token
- 无响应等待：300 秒，可修改

填写北京地域 Token Plan 专属 API Key，保存时新增配置，不覆盖原有 DeepSeek。
当前已是 Token Plan 配置时编辑原配置。密钥仅沿用系统凭据库保存方式。
Responses 消息中的 developer 角色在千问 Chat Completions 请求中转换为 system，
保留指令内容和原有顺序，避免千问返回 invalid_parameter_error。
预设未经真实付费接口调用验证，套餐模型权限以用户控制台为准。

官方参考（2026-09-15 核对）：
- https://help.aliyun.com/zh/model-studio/more-tools
- https://help.aliyun.com/zh/model-studio/token-plan-personal-overview
- https://help.aliyun.com/zh/model-studio/qwen3-8-max

## 千问图片输入

Qwen 协议下，qwen3.8-max（含已登记的预览和日期版本）及 qwen3.8-flash
会向内核声明 text/image 输入能力，并允许桌面附件转换成图片输入。
用户图片转为 Chat Completions 的 image_url 内容；工具返回的图片独立附在
连续工具结果之后，保留工具调用关联，避免把图片编码当成普通文本发送。
其他千问模型不会自动获得图片能力。

本地验证包含附件保存/读取、协议转换，以及固定版本原生内核连接模拟千问接口：
首轮图片和第二轮历史图片均到达模型请求。未调用用户的付费接口。
原生集成测试为 crates/model/tests/kernel_qwen_images.rs，设置
SIMPLE_TEST_KERNEL_MANIFEST 后使用 --ignored 执行。无需重新编译 Codex 内核。

## 超时修复

旧实现将模型配置的 timeout_ms 用于 HTTP 请求总时长；默认 120 秒，
即使模型仍在持续输出也会被截断。模型设置每次保存还会写死为 120000。

新实现限制建立连接（最多 30 秒）和连续读取无数据的等待时间，
不对健康响应设置 HTTP 总时长。原有配置数值保留，设置中可调整为 1–600 秒。
这不取消工具执行的独立时限或内核自身的连接保护。

本地 HTTP 回归覆盖原生 Responses、千问 Chat Completions：持续心跳超过等待
时限仍能完成，静默连接仍会失败，非法事件流仍能区分。不会自动重放失败请求。
