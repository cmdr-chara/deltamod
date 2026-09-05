// Deterministic IPC fixtures. All view, routing, localization and interaction code remains real.
(() => {
  window.__calls=[]; window.__rejectToggle=false;
  const mods=Array.from({length:12},(_,i)=>({uid:'mod-'+i,folder:'mod-'+i,name:['Better battles','Quiet footsteps','French translation','The Lost Chapter'][i%4]+' '+(i+1),description:'A carefully crafted mod for your next playthrough. Compatible with the current game installation.',author:['Community creator'],version:'1.'+i+'.0',size:2048*(i+1),packageID:'toby.deltarune',game:'toby.deltarune',mergeSupport:true,_enabled:i%2===0,_imagePath:'./img/mod-placeholder.png',gamebanana:{supports:true,id:i+1,model:'Mod'},variants:[]}));
  const game={id:'toby.deltarune',pid:'toby.deltarune',name:'DELTARUNE',gamebanana:{id:7243},availableFeatures:[{feat:'steam'}]};
  const invoke=async(channel,args=[])=>{
    window.__calls.push({channel,args});
    if(channel==='toggleModState' && window.__rejectToggle) throw new Error('Fixture write rejected');
    const values={getModList:{modList:mods,errors:[]},getModListFull:mods,getTheme:'base',getSystemIndex:0,getGameInfo:game,getCurrentGameInfo:game,getAvailableGames:[game],getInstallations:[{pid:game.id,index:0,name:'DELTARUNE · Steam',valid:true,issues:[],steam:true},{pid:game.id,index:1,name:'Test installation',valid:true,issues:[],steam:false}],getOS:{platform:'win32'},getModImage:{path:'./img/mod-placeholder.png'},loadedDeltarune:{loaded:true},getGamebananaID:7243,getGamebananaPic:'./img/mod-placeholder.png',getGamebananaUserinfo:{_sName:'Chara'},howManyMods:mods.length,gamebanana_getCollections:[{id:1,name:'My next playthrough'},{id:2,name:'Translations'}],getOfficialProfileSummary:{exists:false},'lifecycle:listProfiles':[],'lifecycle:getInstalledMods':{schemaVersion:1,mods:[],operations:[],recoveries:[]},'lifecycle:getActiveProfile':null,'storage:getUsage':{totalBytes:0,cacheBytes:0,recoveryBytes:0}};
    if(channel==='getThemes')return window.__themes;
    if(channel==='getUniqueFlag') return !['AUDIO','SFX','DYNAMUSIC'].includes(args[0]);
    if(channel==='validateGamebananaToken')return true;
    return values[channel]??false;
  };
  const unavailable=new Set(['isInstallerMode','benchmark:rendererReady','npsCallback']);
  window.deltamodBackend={invoke,invokeOptional:async(c,a,f)=>unavailable.has(c)?f:invoke(c,a),isCommandAvailable:c=>!unavailable.has(c),assetUrl:(scope,path)=>'/'+path.replace(/^web\//,'').replace(/^data\//,'themes/data/')};
  window.electronAPI=window.deltamodBackend;
  window.preloadAPI=new Proxy({}, {get:(o,k)=>cb=>()=>{}});
  window.communityAPI={app:{version:async()=> '2.0.18',openMaintainerProfile:()=>{}},profile:{summary:async()=>({exists:false})},modSources:{providers:async()=>[{id:'gamebanana',name:'GameBanana',browse:true,available:true},{id:'nexus',name:'Nexus Mods',browse:true,available:true},{id:'moddb',name:'ModDB',browse:true,available:true}],browse:async request=>({ok:true,result:{provider:request.provider,items:[],payload:request.url?.includes('TopSubs')?[]:{_aRecords:[],_aMetadata:{_bIsComplete:true,_nRecordCount:0}},sourceUrl:'https://gamebanana.com',hasMore:false}}),nexusStatus:async()=>({authenticated:false}),onProgress:()=>()=>{}},tools:{}};
  window.__themes=[{id:'base',name:'Base Theme',builtIn:true,color:'rgb(205, 68, 81)',soulColor:'#FF0000',background:'ch5.png',mainSong:null,description:'Classic Deltamod'}];
})();
