import { useDialog } from "./useDialog";

export function FirstRunGuide({ onClose, onConfigure }: {
  onClose: () => void;
  onConfigure: () => void;
}) {
  const dialogRef = useDialog<HTMLDivElement>(onClose);
  return (
    <div className="modal-backdrop" role="presentation">
      <div ref={dialogRef} tabIndex={-1} className="first-run-guide" role="dialog"
        aria-modal="true" aria-labelledby="first-run-title" aria-describedby="first-run-description">
        <p className="eyebrow">开始使用</p>
        <h2 id="first-run-title">欢迎使用 Simple</h2>
        <p id="first-run-description">连接模型，打开项目，把开发任务交给 Simple。</p>
        <ol className="first-run-steps">
          <li><span aria-hidden="true">1</span><div><h3>连接你的模型</h3><p>填写接口地址、模型名称和 API Key。密钥保存在本机系统凭据库；使用远程模型时，请求会发送到你配置的接口。</p></div></li>
          <li><span aria-hidden="true">2</span><div><h3>打开一个项目</h3><p>点击左侧「打开项目」，选择要处理的文件夹，再用自然语言描述任务。</p></div></li>
          <li><span aria-hidden="true">3</span><div><h3>从一个小任务开始</h3><p>例如：请阅读项目，告诉我如何启动，先不要修改文件。熟悉后再尝试修改代码。</p></div></li>
        </ol>
        <p className="first-run-caution">这是早期版本，可能存在错误。重要项目请先备份或提交到 Git；开始时建议使用需确认的权限模式，并检查修改结果。</p>
        <div className="first-run-actions">
          <button type="button" className="primary-action" onClick={onConfigure}>开始配置 <span aria-hidden="true">→</span></button>
          <button type="button" className="text-button" onClick={onClose}>稍后再说</button>
        </div>
        <small>以后可以在左下角「设置」中查看使用引导。</small>
      </div>
    </div>
  );
}
