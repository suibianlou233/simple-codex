# assistant-ui 对话界面

依赖：@assistant-ui/react 0.15.20（MIT）。使用本地 ExternalStoreRuntime、Thread.Root/Viewport、MessageProvider/Message.Root、Composer.Root/Input。两份应用使用相同的 conversation.tsx、conversation.css 和执行过程组件。

原有后端快照仍是消息、回合和审批的事实来源。输入框与发送处理保留原生桥接：附件和粘贴块在提交时组装，只有后端确认成功才清空草稿；不使用运行时的乐观清空。任务分支、重新生成、审批和撤销沿用现有回调。消息投影保留流式、失败和分页规则，MessageProvider 随投影同步更新，避免外部运行时刷新前按新索引读取消息。

工作台拥有滚动跟随策略，assistant-ui Viewport 关闭重复自动滚动。项目/任务/模式草稿仍由原状态存储维护。所有模型请求继续走原配置的本地桥接，不接入 assistant-ui 云服务。

## 执行过程

来源：https://github.com/assistant-ui/assistant-ui
固定源码版本：fd31eda583b94d7aec5efb48e0ba384014d34e5c。
上游 reasoning.tsx 与 Radix collapsible 组件使用 Simple 主题和中文标签。保留手动展开优先、流式跟随和关闭内容状态。显示后端 commentary，标为“执行过程”，耗时标为“本轮用时”。

MIT 许可见 apps/desktop/src/components/assistant-ui/LICENSE 和 apps/desktop/public/licenses/assistant-ui.txt。
