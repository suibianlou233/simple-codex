import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useDialog } from "../components/useDialog";
import "./SkillManager.css";

interface Skill { id: string; name: string; description: string; enabled: boolean; issues: string[]; files: number }
interface Candidate { id: string; name: string; description: string }

export function SkillManager({ onClose }: { onClose: () => void }) {
  const ref = useDialog<HTMLElement>(onClose);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [document, setDocument] = useState<{ name: string; text: string }>();
  useEffect(() => {
    let live = true;
    Promise.all([invoke<Skill[]>("skills_library_list"), invoke<Candidate[]>("skills_codex_list")])
      .then(([library, local]) => { if (live) { setSkills(library); setCandidates(local); } })
      .catch((cause: unknown) => { if (live) setError(String(cause)); })
      .finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, []);
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true); setError("");
    try { await action(); setSkills(await invoke<Skill[]>("skills_library_list")); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  };
  const importSkill = (sourceId: string | null) => run(() => invoke("skills_import", { sourceId }));
  return <div className="modal-backdrop"><section ref={ref} className="skill-manager" role="dialog" aria-modal="true" aria-labelledby="skill-manager-title" tabIndex={-1}>
    <header><h2 id="skill-manager-title">技能管理</h2><button type="button" onClick={onClose} aria-label="关闭技能管理">关闭</button></header>
    <p>导入后保存在 Simple 本地，所有模型配置共用。模型会按任务需要选择并读取技能。更改在下一轮任务生效。</p>
    <button type="button" disabled={busy} onClick={() => void importSkill(null)}>从文件夹导入</button>
    {error && <p role="alert">{error}</p>}
    {busy && <p role="status">正在处理…</p>}
    <h3>Simple 技能库</h3>
    {!busy && !skills.length && <p>还没有导入技能。可从下方本机 Codex 技能中选择。</p>}
    {skills.map((skill) => <article key={skill.id}>
      <strong>{skill.name}</strong><span className="skill-status">{skill.enabled ? "已启用" : "已停用"}</span>
      <p>{skill.description}</p>
      <small>{skill.files} 个文件 · {skill.issues.length ? "依赖待检查" : "基础检查通过，使用效果待验证"}</small>
      {skill.issues.length > 0 && <ul>{skill.issues.map((issue) => <li key={issue}>{issue}</li>)}</ul>}
      <div className="skill-actions">
        <button disabled={busy} onClick={() => void run(() => invoke("skills_set_enabled", { id: skill.id, enabled: !skill.enabled }))}>{skill.enabled ? "停用" : skill.issues.length ? "仍启用（依赖待验证）" : "启用"}</button>
        <button disabled={busy} onClick={() => void run(async () => setDocument({ name: skill.name, text: await invoke<string>("skills_document", { id: skill.id }) }))}>查看说明</button>
        <button disabled={busy} onClick={() => void importSkill(`update:${skill.id}`)}>从原位置更新</button>
      </div>
    </article>)}
    {document && <section className="skill-document"><header><h3>{document.name}</h3><button onClick={() => setDocument(undefined)}>收起说明</button></header><pre>{document.text}</pre></section>}
    <h3>本机 Codex 技能</h3>
    <p>只复制你选择的技能文件夹。包含专用工具或额外依赖的技能需要适配。</p>
    {!busy && !candidates.length && <p>未发现本机个人技能，可以手动选择文件夹。</p>}
    {candidates.map((candidate) => <article key={candidate.id}><strong>{candidate.name}</strong><p>{candidate.description}</p><button disabled={busy} onClick={() => void importSkill(candidate.id)}>{skills.some((skill) => skill.name === candidate.name) ? "更新此技能" : "导入"}</button></article>)}
  </section></div>;
}
