// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import { WindowActivation, restoreDialogFocus } from '../src/window-lifecycle.mjs';

test('handoffs before native mount coalesce, including an empty secondary launch', () => {
  const activation = new WindowActivation();
  let calls = 0;
  activation.request(); activation.request();
  assert.equal(calls, 0);
  const unbind = activation.bind(() => calls++);
  assert.equal(calls, 1);
  activation.request();
  assert.equal(calls, 2);
  unbind(); activation.request(); activation.request();
  activation.bind(() => calls++);
  assert.equal(calls, 3);
});
test('OS activation refusal is reported without retrying accepted operations', () => {
  const errors = [];
  const activation = new WindowActivation(message => errors.push(message));
  activation.bind(() => { throw new Error('denied'); });
  assert.doesNotThrow(() => activation.request());
  assert.equal(errors.length, 1);
  activation.dispose(); activation.request();
  activation.bind(() => { throw new Error('must not run'); });
  assert.equal(errors.length, 1);
});
test('stale unmount cannot disconnect a replacement native host', () => {
  const activation = new WindowActivation();
  let calls = 0;
  const old = activation.bind(() => { throw new Error('old'); });
  activation.bind(() => calls++);
  old(); activation.request();
  assert.equal(calls, 1);
});
test('dialog focus restoration rejects missing, removed and invalid native ids', () => {
  const calls = [];
  const renderer = { getElementBounds: id => id === 42 ? {} : null, focusElement: id => calls.push(id) };
  for (const id of [undefined, null, -1, NaN, 0.5, 43]) assert.equal(restoreDialogFocus(renderer, id), false);
  assert.equal(restoreDialogFocus(renderer, 42, 42), false);
  assert.equal(restoreDialogFocus(renderer, 42, 99), true);
  assert.deepEqual(calls, [42]);
  assert.equal(restoreDialogFocus({}, 42), false);
  assert.equal(restoreDialogFocus({ ...renderer, focusElement() { throw Error(); } }, 42), false);
});
