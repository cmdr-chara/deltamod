// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import type { Locale } from './languages.mjs';
export function translate(locale:Locale,key:string,values?:Record<string,string|number>):string;
export function formatBytes(locale:Locale,bytes:number|null):string;
