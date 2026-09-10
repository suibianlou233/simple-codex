import { createContext, useContext, useEffect, useState } from "react";
import { mediaAvailable, mediaBridge } from "../bridge/mediaBridge";

export const AttachmentProjectContext = createContext<string | undefined>(undefined);

export function StoredImage({ reference, name, projectId: explicitProject }: { reference: string; name: string; projectId?: string }) {
  const contextProject = useContext(AttachmentProjectContext);
  const projectId = explicitProject ?? contextProject;
  const [image, setImage] = useState<{ key: string; data?: string }>();
  const key = `${projectId}:${reference}`;
  useEffect(() => {
    let cancelled = false;
    if (!projectId || !mediaAvailable || !/^simple-image:[a-f0-9]{64}$/.test(reference)) return;
    void mediaBridge.readImage(projectId, reference).then(data => {
      if (!cancelled) setImage({ key, data });
    }, () => { if (!cancelled) setImage({ key }); });
    return () => { cancelled = true; };
  }, [projectId, reference, key]);
  return image?.key === key && image.data
    ? <img className="stored-image" src={image.data} alt={name || "附件图片"} loading="lazy" />
    : <span className="image-placeholder">{image?.key === key ? "图片不可用" : "图片"}：{name}</span>;
}
