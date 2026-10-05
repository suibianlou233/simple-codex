import type {ContextMode,DesktopSnapshot,TaskSummary} from "../bridge/types";
export const taskMode=(task:TaskSummary):ContextMode=>task.contextMode??"simple";
export const contextKey=(projectId:string|null|undefined,mode:ContextMode)=>`${projectId??"none"}:${mode}`;
export function snapshotForMode(snapshot:DesktopSnapshot,mode:ContextMode):DesktopSnapshot {
 const tasks=snapshot.tasks.filter(t=>taskMode(t)===mode),ids=new Set(tasks.map(t=>t.id));
 return {...snapshot,tasks,timeline:snapshot.timeline.filter(t=>ids.has(t.taskId)),actions:snapshot.actions.filter(t=>ids.has(t.taskId)),turns:snapshot.turns.filter(t=>ids.has(t.taskId)),contextUsage:snapshot.contextUsage.filter(t=>ids.has(t.taskId)),activeTaskId:ids.has(snapshot.activeTaskId??"")?snapshot.activeTaskId:null,activeTurnId:snapshot.turns.some(t=>t.id===snapshot.activeTurnId&&ids.has(t.taskId))?snapshot.activeTurnId:null};
}
