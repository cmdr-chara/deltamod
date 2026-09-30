// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export const GPUIX_VERSION: '2.0.18';
export const GPUIX_UPDATE_ENDPOINT: string;
export const GPUIX_UPDATE_PUBLIC_KEY: string;
export type UpdateStatus = 'idle'|'checking'|'available'|'installing'|'installed'|'current'|'error';
export interface UpdateLike { version:string; downloadAndInstall():Promise<unknown> }
export class UpdateRuntime {
  constructor(checkFn?: (version:string, options:Record<string,unknown>)=>Promise<UpdateLike|null>);
  state:{status:UpdateStatus;update:UpdateLike|null;error:string};
  subscribe:(listener:()=>void)=>()=>void;
  getSnapshot:()=>{status:UpdateStatus;update:UpdateLike|null;error:string};
  check():Promise<boolean>;
  install():Promise<boolean>;
  dispose():void;
}
