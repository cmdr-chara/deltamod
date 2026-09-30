// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import type { Options } from './options.mjs';
import type { Handoff, HandoffReceipt } from './handoffs.mjs';
export interface Instance {primary:boolean;accepted:number;dispose:()=>void}
export function startSingleInstance(options:Options,items:Handoff[],receive:(items:Handoff[])=>HandoffReceipt):Promise<Instance>;
