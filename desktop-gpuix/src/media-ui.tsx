// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { createContext, useContext, useEffect, useRef, useState } from 'react';
import { motion, useGpuixRequired, type ImgInstance } from '@gpuix/react';
import { loadThemeMedia, mediaTools, NativeThemePlayer, VIDEO } from './theme-media.mjs';
import { useMessages } from './i18n.js';
import { Action, Label, Palette, row, column } from './ui.js';
export const ThemeResources = createContext('');

export function NativeMediaPreview({ themeId }: { themeId: string }) {
  const resources=useContext(ThemeResources);
  const preferences=useContext(Palette);
  const renderer=useGpuixRequired();
  const image=useRef<ImgInstance|null>(null);
  const player=useRef<NativeThemePlayer|null>(null);
  const [state,setState]=useState('unavailable');
  const [error,setError]=useState('');
  const [muted,setMuted]=useState(true);
  const [volume,setVolume]=useState(50);
  const [seconds,setSeconds]=useState(0);
  const [cue,setCue]=useState<number|null>(null);
  const [cueReached,setCueReached]=useState(false);
  const [soulColor,setSoulColor]=useState('#ff0000');
  const [video,setVideo]=useState(false);
  const t=useMessages();
  useEffect(()=>{
    let active=true;setState('unavailable');setError('');setSeconds(0);setCueReached(false);setVideo(false);setCue(null);
    try {
      const theme=loadThemeMedia(resources,themeId);
      setCue(theme.cue);setSoulColor(theme.soulColor);setVideo(!!theme.video);
      if(!theme.video&&!theme.audio) return;
      const tools=mediaTools(resources);
      const native=new NativeThemePlayer(theme,tools,{
        onFrame:pixels=>{if(active&&image.current)renderer.setImagePixels(image.current.id,VIDEO.width,VIDEO.height,pixels,'bgra');},
        onState:(value,message)=>{if(active){setState(value);setError(message);}},
        onTime:(value,reached)=>{if(active){setSeconds(value);setCueReached(reached);}},
      });
      player.current=native;setState('stopped');
      return()=>{active=false;player.current=null;native.dispose();};
    } catch { setError('Native media tools or bundled assets are unavailable.'); }
    return()=>{active=false;};
  },[resources,themeId,renderer]);
  // Never keep audio or video running after accessibility/volume preferences
  // change. Restart is an explicit user action with the new settings.
  useEffect(()=>{void player.current?.stop();},[preferences.reducedMotion,muted,volume]);
  const running=state==='running'||state==='starting';
  return <div role="region" aria-label={t('Native theme playback')} style={{...column,gap:8,maxHeight:310,overflowY:'scroll'}}>
    {video&&!preferences.reducedMotion&&running&&<img ref={image} alt={t('Native theme video')} objectFit="contain" style={{width:'100%',height:180}}/>}
    <div style={row}>
      <Action testId="media-play" disabled={state==='unavailable'||running} onClick={()=>{void player.current?.play({muted,volume,reducedMotion:preferences.reducedMotion});}}>{t('Play media')}</Action>
      <Action testId="media-stop" disabled={!running} onClick={()=>{void player.current?.stop();}}>{t('Stop media')}</Action>
      <Action testId="media-audio" onClick={()=>setMuted(value=>!value)}>{t(muted?'Enable audio':'Mute audio')}</Action>
    </div>
    {!muted&&<div style={row}>
      <Label>{t('Volume')}: {volume}%</Label>
      <Action disabled={volume===0} onClick={()=>setVolume(value=>Math.max(0,value-10))}>{t('Quieter')}</Action>
      <Action disabled={volume===100} onClick={()=>setVolume(value=>Math.min(100,value+10))}>{t('Louder')}</Action>
    </div>}
    {cue!==null&&<div style={row}>
      <Action disabled={state==='unavailable'||running} onClick={()=>{void player.current?.play({muted,volume,reducedMotion:preferences.reducedMotion,fromCue:true});}}>{t('Play from synchronization cue')}</Action>
      <motion.div initial={false} animate={{opacity:cueReached?1:0.45}} transition={{duration:preferences.reducedMotion?0:0.18}}><text aria-label={t(cueReached?'Synchronization cue reached':'Synchronization cue pending')}
        style={{fontSize:22,color:soulColor}}>♥</text></motion.div>
    </div>}
    <Label muted size={12}>{t('Playback position')}: {Math.floor(seconds)}s</Label>
    {preferences.reducedMotion&&<Label muted size={12}>{t('Reduced motion keeps video and animated effects off. Audio remains optional.')}</Label>}
    {error&&<div role="alert"><Label muted size={12}>{t(error)}</Label></div>}
    <Label muted size={12}>{t('Native media preview. Audio/video drift and installed codec readiness still require platform validation.')}</Label>
  </div>;
}
