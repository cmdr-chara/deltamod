// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { spawn } from 'node:child_process';
import { performance } from 'node:perf_hooks';
import { themePlaybackCoordinator } from './media-owner.mjs';
import { checkedArtifact, readJsonResource, regularFile, targetFor } from './runtime-layout.mjs';

export const VIDEO = Object.freeze({ width: 640, height: 360, fps: 24, bytes: 640 * 360 * 4 });
const name = value => typeof value === 'string' && value.length <= 255
  && /^[^/\\:\x00-\x1f\x7f]+$/.test(value) && value !== '.' && value !== '..';
const media = (root, directory, value, extensions, limit) => {
  if (value === undefined || value === null || value === '') return null;
  if (!name(value) || !extensions.test(value)) throw new Error('Unsupported bundled theme media filename.');
  return regularFile(root, `web/themes/${directory}/${value}`, limit);
};

export function loadThemeMedia(root, themeId) {
  if (typeof themeId !== 'string' || !/^[A-Za-z0-9_-]{1,64}$/.test(themeId)) throw new Error('Invalid built-in theme ID.');
  const theme = readJsonResource(root, `web/themes/data/${themeId}.theme.json`);
  if (!theme || theme.id !== themeId) throw new Error('Built-in theme identity mismatch.');
  const video = media(root, 'video', theme.backgroundVideo, /\.(mp4|webm|mov)$/i, 256 * 1024 * 1024);
  const song = media(root, 'mus', theme.mainSong, /\.(mp3|ogg|wav|flac|m4a)$/i, 64 * 1024 * 1024);
  const cue = theme.bootSyncTime ?? null;
  if (cue !== null && (!Number.isFinite(cue) || cue < 0 || cue > 3600)) throw new Error('Invalid theme synchronization cue.');
  return Object.freeze({ themeId, video, audio: theme.videoHasAudio === true && video ? video : song, cue,
    soulColor: typeof theme.soulColor === 'string' && /^#[0-9a-f]{6}$/i.test(theme.soulColor) ? theme.soulColor : '#ff0000' });
}

/** The manifest belongs to the release inputs. Never search PATH or download a
 * codec at runtime. Hashes bind staged bytes, not publisher signing authority. */
export function mediaTools(root) {
  const target = targetFor();
  const manifest = readJsonResource(root, 'native/media.json');
  if (!manifest || manifest.schemaVersion !== 1 || manifest.target !== target.id
    || typeof manifest.source !== 'string' || !manifest.source.startsWith('https://')
    || typeof manifest.license !== 'string' || !manifest.license.trim()) throw new Error('Invalid native media provenance.');
  regularFile(root, manifest.license, 8 * 1024 * 1024);
  return Object.freeze({ ffmpeg: checkedArtifact(root, manifest.ffmpeg, target.id), ffplay: checkedArtifact(root, manifest.ffplay, target.id) });
}

/** One fixed-size frame in memory, regardless of pipe fragmentation. */
export class FrameAssembler {
  constructor(deliver) { this.buffer = Buffer.alloc(VIDEO.bytes); this.offset = 0; this.deliver = deliver; }
  push(chunk) {
    if (!Buffer.isBuffer(chunk) || chunk.length > 4 * 1024 * 1024) throw new Error('Native video output exceeded its frame budget.');
    let start = 0;
    while (start < chunk.length) {
      const count = Math.min(chunk.length - start, VIDEO.bytes - this.offset);
      chunk.copy(this.buffer, this.offset, start, start + count);this.offset += count;start += count;
      if (this.offset === VIDEO.bytes) { this.offset = 0;this.deliver(Buffer.from(this.buffer)); }
    }
  }
}
export function playbackPlan(theme, { muted = true, volume = 50, reducedMotion = true, fromCue = false, repeat = false } = {}) {
  if ([muted, reducedMotion, fromCue, repeat].some(value => typeof value !== 'boolean')) throw new Error('Invalid native playback options.');
  if (!Number.isInteger(volume) || volume < 0 || volume > 100) throw new Error('Invalid native audio volume.');
  const offset = fromCue ? theme.cue ?? 0 : 0;
  const video = !reducedMotion && theme.video;
  const audio = !muted && volume > 0 && theme.audio;
  if (!video && !audio) throw new Error('Enable audio or turn off reduced motion to play this theme.');
  const seek = offset > 0 ? ['-ss', String(offset)] : [];
  return { offset,
    video: video ? ['-hide_banner', '-loglevel', 'error', '-nostdin', '-threads', '2', '-filter_threads', '1', '-protocol_whitelist', 'file,pipe', '-re', ...(repeat ? ['-stream_loop', '-1'] : []), ...seek,
      '-i', video, '-map', '0:v:0', '-an', '-sn', '-dn', '-t', '3600', '-vf',
      `scale=${VIDEO.width}:${VIDEO.height}:force_original_aspect_ratio=decrease,pad=${VIDEO.width}:${VIDEO.height}:(ow-iw)/2:(oh-ih)/2,fps=${VIDEO.fps}`,
      '-pix_fmt', 'bgra', '-f', 'rawvideo', 'pipe:1'] : null,
    audio: audio ? ['-hide_banner', '-loglevel', 'error', '-nostats', '-nodisp', '-autoexit',
      '-protocol_whitelist', 'file,pipe', ...(repeat ? ['-loop', '0'] : []), ...seek, '-i', audio, '-vn', '-sn', '-t', '3600', '-volume', String(volume)] : null,
  };
}

/** Native decoders remain owned child processes. No browser media, shell command,
 * dynamic filter expressions, remote URLs or automatic retry. A stopped/changed
 * theme cannot paint stale frames or continue playing audio. Preview A/V starts
 * from the same seek offset, but separate decoder clocks are not sample-locked. */
export class NativeThemePlayer {
  constructor(theme, tools, { onFrame = () => {}, onState = () => {}, onTime = () => {}, spawnImpl = spawn, coordinator = themePlaybackCoordinator } = {}) {
    this.coordinator=coordinator;
    this.theme=theme;this.tools=tools;this.onFrame=onFrame;this.onState=onState;this.onTime=onTime;this.spawn=spawnImpl;
    this.children=new Set();this.sequence=0;this.disposed=false;this.blocked=false;this.timer=null;this.stopPromise=Promise.resolve();
    this.exit=()=>{ for(const child of this.children) { try { child.kill('SIGKILL'); } catch {} } };
    process.once('exit',this.exit);
  }
  async play(options = {}) {
    const sequence=++this.sequence;
    try {
      return await this.coordinator.claim(this, async () => this.startPlayback(options, sequence));
    } catch (error) {
      if (!this.disposed && sequence === this.sequence) this.onState('error', error.message);
      return false;
    }
  }
  async startPlayback(options, sequence) {
    await this.stopChildren();
    if(this.disposed || sequence!==this.sequence) return false;
    if(this.blocked || this.children.size) { this.onState('error','A previous native player could not be stopped. Restart the application.');return false; }
    let plan;
    try { plan=playbackPlan(this.theme,options); }
    catch(error) { this.onState('error',error.message);return false; }
    const fail=()=>{
      if(sequence!==this.sequence || this.disposed) return;
      this.sequence++;void this.stopChildren();this.onState('error','Native theme playback failed.');
    };
    this.onState('starting','');
    let frames=0;let lastVideo=performance.now();let firstFrame=false;const started=performance.now();
    const launch=(executable,args,isVideo)=>{
      const child=this.spawn(executable,args,{shell:false,windowsHide:true,stdio:['ignore',isVideo?'pipe':'ignore','pipe']});
      this.children.add(child);
      child.once('error',()=>{if(child.pid==null)this.children.delete(child);fail();});
      child.once('close',(code)=>{
        this.children.delete(child);
        if(sequence!==this.sequence || this.disposed) return;
        if(code!==0) { fail();return; }
        if(isVideo || !plan.video) { void this.stop(); }
      });
      // Drain diagnostics without retaining media paths, unbounded output or secrets.
      let errors=0;child.stderr?.on('data',chunk=>{errors+=chunk.length;if(errors>16*1024)fail();});
      if(isVideo) {
        const assembler=new FrameAssembler(pixels=>{
          if(sequence!==this.sequence || this.disposed) return;
          try { this.onFrame(pixels); } catch { fail();return; }
          frames++;lastVideo=performance.now();
          if(!firstFrame) {firstFrame=true;this.onState('running','');}
        });
        child.stdout?.on('data',chunk=>{try{assembler.push(chunk);}catch{fail();}});
      } else if(!plan.video) child.once('spawn',()=>{if(sequence===this.sequence&&!this.disposed)this.onState('running','');});
    };
    try {
      if(plan.video) launch(this.tools.ffmpeg,plan.video,true);
      if(plan.audio) launch(this.tools.ffplay,plan.audio,false);
    } catch { fail();return false; }
    this.timer=setInterval(()=>{
      if(sequence!==this.sequence || this.disposed) return;
      const now=performance.now();
      if(plan.video && now-lastVideo>10000) {fail();return;}
      if(now-started>3600*1000) {void this.stop();return;}
      const seconds=plan.offset+(plan.video ? Math.max(0,frames-1)/VIDEO.fps : (now-started)/1000);
      try { this.onTime(seconds,this.theme.cue!==null && seconds>=this.theme.cue); } catch { fail(); }
    },250);
    this.timer.unref();
    return true;
  }
  stopChildren() {
    clearInterval(this.timer);this.timer=null;
    const children=[...this.children];
    // Wait for actual close, not ChildProcess.killed (which only means a signal
    // was sent). This prevents rapid Play/Stop changes from overlapping audio.
    const closing=children.map(child=>new Promise(resolve=>{
      let finished=false;
      const done=()=>{if(finished)return;finished=true;clearTimeout(force);clearTimeout(deadline);this.children.delete(child);resolve();};
      const force=setTimeout(()=>{try{child.kill('SIGKILL');}catch{}},1000);
      const deadline=setTimeout(()=>{
        this.blocked=true;finished=true;clearTimeout(force);
        child.once('close',()=>this.children.delete(child));
        resolve();
      },3000);
      child.once('close',done);child.once('error',()=>{if(child.pid==null)done();});
      // A failed kill is not evidence that the process exited.
      try{child.kill();}catch{}
    }));
    this.stopPromise=Promise.all([this.stopPromise,...closing]).then(()=>{});
    return this.stopPromise;
  }
  async stop() { this.sequence++;await this.stopChildren();if(!this.disposed)this.onState(this.blocked?'error':'stopped',this.blocked?'A previous native player could not be stopped. Restart the application.':''); }
  dispose() { if(this.disposed)return;this.disposed=true;this.sequence++;void this.stopChildren().then(()=>{if(!this.children.size)process.off('exit',this.exit);}); }
}
