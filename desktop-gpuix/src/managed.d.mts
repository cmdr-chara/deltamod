// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import type { BackendClient } from './contracts.js';
export interface ManagedMod { instanceId:string; installationId:string; modId:string; name:string; version:string; provider:string; files:number }
export interface ManagedCatalog { mods:ManagedMod[]; verification:Record<string,unknown>[]; journals:Record<string,unknown>[]; operations:Record<string,unknown>[]; errors:unknown[]; health:unknown[] }
export interface ManagedState { catalog:ManagedCatalog|null; installations:unknown[]; game:Record<string,unknown>|null; credentials:Record<string,unknown>|null; enabledIds:string[]; loading:boolean; busy:string; error:string; lastOperation:unknown }
export function normalizeManagedCatalog(value:unknown):ManagedCatalog;
export function operationId(prefix?:string):string;
export class ManagedRuntime {
 constructor(bridge:BackendClient);
 state:ManagedState; subscribe:(listener:()=>void)=>()=>void; getSnapshot:()=>ManagedState;
 initialize():Promise<boolean>; refresh():Promise<boolean>; importArchive(path:string,replaceExisting?:boolean):Promise<boolean>;
 toggle(uid:string,enabled:boolean):Promise<boolean>; variant(uid:string,variant:string):Promise<boolean>;
 verify(mod:ManagedMod):Promise<boolean>; repair(mod:ManagedMod):Promise<boolean>; uninstall(mod:ManagedMod):Promise<boolean>;
 restore(installationId:string):Promise<boolean>; patch(selected:string[]):Promise<boolean>; cancelPatch():Promise<boolean>;
 hashes():Promise<boolean>; launch():Promise<boolean>; clearCredential(kind:string):Promise<boolean>; dispose():void;
}
