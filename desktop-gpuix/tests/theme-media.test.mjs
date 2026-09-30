// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { spawnSync } from 'node:child_process';
import { FrameAssembler, NativeThemePlayer, VIDEO, loadThemeMedia, playbackPlan } from '../src/theme-media.mjs';
const theme={themeId:'base',video:'/bundled/theme.mp4',audio:'/bundled/theme.mp4',cue:5.6,soulColor:'#ff0000'};
function fakeSpawner() {
  const children=[];
  return {children,spawn(executable,args,options) {
    const child=new EventEmitter();child.stdout=new PassThrough();child.stderr=new PassThrough();child.pid=1234+children.length;
    child.kill=(signal='SIGTERM')=>{child.signal=signal;queueMicrotask(()=>child.emit('close',null));return true;};
    children.push({child,executable,args,options});queueMicrotask(()=>child.emit('spawn'));return child;
  }};
}
test('frames tolerate fragmentation without retaining an unbounded pipe buffer',()=>{
  const frames=[];const parser=new FrameAssembler(value=>frames.push(value));
  const source=Buffer.alloc(VIDEO.bytes*2,7);source[VIDEO.bytes]=8;
  parser.push(source.subarray(0,13));parser.push(source.subarray(13,VIDEO.bytes+17));parser.push(source.subarray(VIDEO.bytes+17));
  assert.equal(frames.length,2);assert.equal(frames[0].length,VIDEO.bytes);assert.equal(frames[0][0],7);assert.equal(frames[1][0],8);
  assert.equal(parser.offset,0);assert.equal(parser.buffer.length,VIDEO.bytes);
  assert.throws(()=>parser.push(Buffer.alloc(4*1024*1024+1)),/budget/);
});
test('reduced motion, mute and cue seeking become explicit decoder arguments',()=>{
  assert.throws(()=>playbackPlan(theme),/Enable audio/);
  const audio=playbackPlan(theme,{muted:false,reducedMotion:true,fromCue:true,volume:40});
  assert.equal(audio.video,null);assert.equal(audio.offset,5.6);assert.ok(audio.audio.includes('5.6'));
  assert.deepEqual(audio.audio.slice(-2),['-volume','40']);
  const video=playbackPlan(theme,{reducedMotion:false});assert.equal(video.audio,null);
  assert.ok(video.video.includes('file,pipe'));assert.ok(video.video.includes('bgra'));
  for(const volume of [-1,101,NaN,0.5])assert.throws(()=>playbackPlan(theme,{volume}));
});
test('theme records cannot escape bundled directories or select remote media',()=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'gpuix-media-'));
  try {
    fs.mkdirSync(path.join(root,'web/themes/data'),{recursive:true});
    const save=value=>fs.writeFileSync(path.join(root,'web/themes/data/base.theme.json'),JSON.stringify(value));
    save({id:'base',mainSong:'https://example.test/song.mp3'});assert.throws(()=>loadThemeMedia(root,'base'));
    save({id:'base',backgroundVideo:'../clip.mp4'});assert.throws(()=>loadThemeMedia(root,'base'));
    save({id:'another'});assert.throws(()=>loadThemeMedia(root,'base'),/identity/);
    save({id:'base',bootSyncTime:3601});assert.throws(()=>loadThemeMedia(root,'base'),/cue/);
    save({id:'base'});assert.equal(loadThemeMedia(root,'base').video,null);
    assert.throws(()=>loadThemeMedia(root,'../base'));
  } finally {fs.rmSync(root,{recursive:true,force:true});}
});
test('players own their processes, stop audio and ignore frames after stop',async()=>{
  const fake=fakeSpawner();const frames=[];const states=[];
  const player=new NativeThemePlayer(theme,{ffmpeg:'/verified/ffmpeg',ffplay:'/verified/ffplay'},{spawnImpl:fake.spawn,onFrame:v=>frames.push(v),onState:s=>states.push(s)});
  try {
    assert.equal(await player.play({muted:false,reducedMotion:false}),true);
    assert.equal(fake.children.length,2);
    assert.ok(fake.children.every(({options})=>options.shell===false&&options.windowsHide===true));
    fake.children[0].child.stdout.write(Buffer.alloc(VIDEO.bytes));assert.equal(frames.length,1);assert.ok(states.includes('running'));
    await player.stop();assert.equal(player.children.size,0);
    fake.children[0].child.stdout.write(Buffer.alloc(VIDEO.bytes));assert.equal(frames.length,1);
    assert.ok(fake.children.every(({child})=>child.signal));
  } finally {player.dispose();await player.stopPromise;}
});
test('rapid Play changes invalidate the old generation rather than overlap players',async()=>{
  const fake=fakeSpawner();const player=new NativeThemePlayer(theme,{ffmpeg:'/verified/ffmpeg',ffplay:'/verified/ffplay'},{spawnImpl:fake.spawn});
  try {
    const first=player.play({reducedMotion:false});const second=player.play({muted:false,reducedMotion:true});
    assert.equal(await first,false);assert.equal(await second,true);
    assert.equal(fake.children.length,1);assert.equal(fake.children[0].executable,'/verified/ffplay');
  } finally {player.dispose();await player.stopPromise;}
});
test('decoder failures remain errors and never restart automatically',async()=>{
  const fake=fakeSpawner();const states=[];const player=new NativeThemePlayer(theme,{ffmpeg:'/verified/ffmpeg'},{spawnImpl:fake.spawn,onState:(s,e)=>states.push([s,e])});
  try {
    await player.play({reducedMotion:false});fake.children[0].child.emit('error',new Error('private media path'));
    await player.stopPromise;assert.equal(fake.children.length,1);
    assert.ok(states.some(([s,e])=>s==='error'&&e==='Native theme playback failed.'));
    assert.ok(states.every(([,e])=>!e.includes('private media path')));
  } finally {player.dispose();await player.stopPromise;}
});
// Opt in to one local decoder smoke. This validates raw-frame compatibility,
// not a GPU paint, audio device, codec redistribution license or packaged app.
test('real ffmpeg decodes a generated clip through the application frame pipeline',{skip:!process.env.GPUIX_TEST_FFMPEG},()=>{
  const executable=process.env.GPUIX_TEST_FFMPEG;
  assert.ok(path.isAbsolute(executable));
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'gpuix-decode-'));
  try {
    const clip=path.join(root,'test.mp4');
    const made=spawnSync(executable,['-v','error','-f','lavfi','-i','color=size=64x36:rate=24:duration=0.25','-c:v','mpeg4',clip],{timeout:10000,shell:false});
    assert.equal(made.status,0,made.stderr?.toString());
    const plan=playbackPlan({...theme,video:clip},{reducedMotion:false});
    const decoded=spawnSync(executable,plan.video,{timeout:10000,shell:false,maxBuffer:16*1024*1024});
    assert.equal(decoded.status,0,decoded.stderr?.toString());
    let count=0;const parser=new FrameAssembler(()=>count++);
    for(let start=0;start<decoded.stdout.length;start+=65536)parser.push(decoded.stdout.subarray(start,start+65536));
    assert.ok(count>=1);assert.equal(parser.offset,0);
  } finally {fs.rmSync(root,{recursive:true,force:true});}
});
