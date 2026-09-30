// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import type { AppModel } from './model.mjs';
import type { DesktopFeatures } from './features.mjs';
import type { ManagedRuntime } from './managed.mjs';
export type Handoff = Readonly<{kind:'preview'|'protocol'|'archive';value:string}>;
export type HandoffItem = Handoff & Readonly<{id:string}>;
export interface HandoffReceipt {accepted:number;duplicate:number}
export interface HandoffState {items:HandoffItem[];reviewing:boolean;error:string}
export const MAX_HANDOFF_BYTES:number;
export const MAX_PENDING_HANDOFFS:number;
export function protocolLink(raw:unknown):string;
export function archivePath(raw:unknown,platform?:NodeJS.Platform):string;
export function normalizeHandoff(value:unknown,platform?:NodeJS.Platform):Handoff;
export class HandoffInbox {
  state:HandoffState;
  subscribe:(listener:()=>void)=>()=>void;
  getSnapshot:()=>HandoffState;
  receive(items:unknown[]):HandoffReceipt;
  discard(id:string):void;
  review(features:DesktopFeatures,managed:ManagedRuntime,model:AppModel):Promise<boolean>;
  dispose():void;
}
