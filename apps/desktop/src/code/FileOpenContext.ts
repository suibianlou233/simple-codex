import { createContext } from "react";
export const FileOpenContext = createContext<((path:string)=>void)|undefined>(undefined);
