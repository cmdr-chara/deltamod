// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import { nativeLocales, nativeMessages, nativeMessage } from '../src/native-messages.mjs';
test('every new native message is present in all eight supported languages', () => {
  const keys=Object.keys(nativeMessages.en);
  assert.equal(nativeLocales.length,8);
  for(const locale of nativeLocales) {
    assert.deepEqual(Object.keys(nativeMessages[locale]),keys);
    for(const key of keys) {
      const value=nativeMessage(locale,key);
      assert.equal(typeof value,'string');assert.ok(value.trim());
      assert.doesNotMatch(value,/[<>\u0000-\u0008]/);
    }
  }
});
test('unknown keys, prototype names and locales leave source catalogue fallback intact',()=>{
  for(const locale of ['en','it','missing','__proto__']) {
    assert.equal(nativeMessage(locale,'constructor'),undefined);
    assert.equal(nativeMessage(locale,'An existing source message'),undefined);
  }
  assert.equal(nativeMessage('__proto__','Unavailable'),undefined);
});
