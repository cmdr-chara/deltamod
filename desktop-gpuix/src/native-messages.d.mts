// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export const nativeLocales: readonly string[];
export const nativeMessages: Readonly<Record<string, Readonly<Record<string,string>>>>;
export function nativeMessage(locale:string,key:string):string|undefined;
