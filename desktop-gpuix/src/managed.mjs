// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
const record=value=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const text=(value,max=512)=>typeof value==='string'?[...value.replace(/[\u0000-\u001f\u007f]/g,'')].slice(0,max).join(''):'';
const message=error=>error instanceof Error?error.message:String(error);
const idle=()=>({catalog:null,installations:[],game:null,credentials:null,controller:{supported:false,active:false},protocol:{status:'idle',raw:'',intent:null,error:''},enabledIds:[],loading:false,busy:'',error:'',lastOperation:null});

export function normalizeManagedCatalog(value){
  if(!record(value)||!Array.isArray(value.installedMods)||value.installedMods.length>2000
    ||!Array.isArray(value.verificationResults)||!Array.isArray(value.lifecycleJournals)||!Array.isArray(value.operationRecords)){
    throw new Error('Invalid managed lifecycle catalogue.');
  }
  const mods=value.installedMods.map(item=>{
    if(!record(item)) throw new Error('Invalid installed mod record.');
    const instanceId=text(item.instanceId,256), installationId=text(item.installationId,256);
    if(!instanceId||!installationId) throw new Error('Invalid installed mod identity.');
    return {
      instanceId, installationId, modId:text(item.modId,256), name:text(item.displayName)||instanceId,
      version:item.version==null?'':text(item.version,256),
      provider:record(item.provider)?text(item.provider.providerId??item.provider.provider,80):'',
      files:Array.isArray(item.files)?item.files.length:0,
    };
  });
  const verification=value.verificationResults.slice(0,2000).filter(record);
  const journals=value.lifecycleJournals.slice(0,100).filter(record);
  return {mods,verification,journals,operations:value.operationRecords.slice(0,100).filter(record),
    errors:Array.isArray(value.errors)?value.errors.slice(0,64):[],health:Array.isArray(value.gameHealthReports)?value.gameHealthReports.slice(0,64):[]};
}
export function operationId(prefix='gpui'){
  const bytes=new Uint32Array(4);
  crypto.getRandomValues(bytes);
  return `${prefix}-${[...bytes].map(value=>value.toString(16).padStart(8,'0')).join('')}`;
}
export class ManagedRuntime {
  constructor(bridge){
    this.bridge=bridge; this.listeners=new Set(); this.disposed=false; this.sequence=0; this.state=idle();
    this.subscribe=listener=>{this.listeners.add(listener);return()=>this.listeners.delete(listener);};
    this.getSnapshot=()=>this.state;
  }
  update(patch){if(this.disposed)return;this.state={...this.state,...patch};this.listeners.forEach(listener=>listener());}
  async initialize(){return this.refresh();}
  async refresh(){
    const sequence=++this.sequence; this.update({loading:true,error:''});
    try{
      const [catalog,installations,game,credentials,states,controller]=await Promise.all([
        this.bridge.request('managed.catalog'),this.bridge.request('managed.installations'),
        this.bridge.request('managed.game.info').catch(()=>null),this.bridge.request('managed.credentials.status').catch(()=>null),
        this.bridge.request('managed.mod.states').catch(()=>({enabled:[]})),
        this.bridge.request('managed.controller.status').catch(()=>({supported:false,active:false}))
      ]);
      if(sequence!==this.sequence||this.disposed)return false;
      const normalized=normalizeManagedCatalog(catalog);
      if(!Array.isArray(installations)||installations.length>256) throw new Error('Invalid managed installation list.');
      const enabledIds=record(states)&&Array.isArray(states.enabled)?states.enabled.filter(value=>typeof value==='string'&&value.length<=256).slice(0,2000):[];
      this.update({catalog:normalized,installations,game:record(game)?game:null,credentials:record(credentials)?credentials:null,
        controller:record(controller)?{supported:controller.supported===true,active:controller.active===true}:{supported:false,active:false},
        enabledIds,loading:false});
      return true;
    }catch(error){if(sequence===this.sequence)this.update({loading:false,error:message(error)});return false;}
  }
  async mutation(command,args={}){
    if(this.disposed||this.state.busy)return false;
    this.update({busy:command,error:''});
    try{
      const result=await this.bridge.request(command,args);
      if(this.disposed)return false;
      this.update({busy:'',lastOperation:result});
      await this.refresh();
      return true;
    }catch(error){this.update({busy:'',error:message(error)});return false;}
  }
  importArchive(path,replaceExisting=false){return this.mutation('managed.importArchive',{path,replaceExisting});}
  toggle(uid,enabled){return this.mutation('managed.mod.toggle',{uid,enabled});}
  variant(uid,variant){return this.mutation('managed.mod.variant',{uid,variant});}
  verify(mod){return this.mutation('managed.mod.verify',{installationId:mod.installationId,instanceId:mod.instanceId});}
  repair(mod){return this.mutation('managed.mod.repair',{installationId:mod.installationId,instanceId:mod.instanceId,operationId:operationId('repair')});}
  uninstall(mod){return this.mutation('managed.mod.uninstall',{installationId:mod.installationId,instanceId:mod.instanceId,operationId:operationId('uninstall')});}
  restore(installationId){return this.mutation('managed.restore',{installationId,operationId:operationId('restore')});}
  patch(selected){return this.mutation('managed.patch.run',{selected:[...new Set(selected)].slice(0,1000)});}
  async cancelPatch(){
    if(this.disposed||this.state.busy!=='managed.patch.run')return false;
    try{return (await this.bridge.request('managed.patch.cancel'))===true;}
    catch(error){this.update({error:message(error)});return false;}
  }
  hashes(){return this.mutation('managed.hashes');}
  launch(){return this.mutation('managed.game.launch');}
  clearCredential(kind){return this.mutation('managed.credentials.clear',{kind});}
  loginNexus(){return this.mutation('managed.nexus.login');}
  async cancelNexus(){
    if(this.disposed||this.state.busy!=='managed.nexus.login')return false;
    try{return (await this.bridge.request('managed.nexus.cancel'))===true;}
    catch(error){this.update({error:message(error)});return false;}
  }
  controllerStart(){return this.mutation('managed.controller.start');}
  controllerStop(){return this.mutation('managed.controller.stop');}
  async reviewProtocol(raw){
    if(this.disposed||this.state.busy||typeof raw!=='string'||raw.length>8192)return false;
    try{
      const intent=await this.bridge.request('managed.protocol.review',{raw});
      if(!record(intent)||!['import','launch'].includes(intent.kind)||!Number.isSafeInteger(intent.itemId)||intent.itemId<=0
        ||(intent.kind==='import'&&(!Number.isSafeInteger(intent.fileId)||intent.fileId<=0))) throw new Error('Invalid protocol review.');
      this.update({protocol:{status:'reviewed',raw,intent,error:''}});
      return true;
    }catch(error){this.update({protocol:{status:'error',raw:'',intent:null,error:message(error)}});return false;}
  }
  dismissProtocol(){this.update({protocol:{status:'idle',raw:'',intent:null,error:''}});}
  async confirmProtocol(replaceExisting=false){
    const pending=this.state.protocol;
    if(this.disposed||this.state.busy||pending.status!=='reviewed'||pending.intent?.kind!=='import')return false;
    this.update({busy:'managed.protocol.import',protocol:{...pending,status:'importing',error:''}});
    try{
      const result=await this.bridge.request('managed.protocol.import',{raw:pending.raw,replaceExisting:replaceExisting===true});
      if(this.disposed)return false;
      this.update({busy:'',lastOperation:result,protocol:{status:'complete',raw:'',intent:pending.intent,error:''}});
      await this.refresh();
      return true;
    }catch(error){
      this.update({busy:'',protocol:{...pending,status:'error',raw:'',error:message(error)},error:message(error)});
      return false;
    }
  }
  async cancelProtocol(){
    if(this.disposed||this.state.busy!=='managed.protocol.import')return false;
    try{return (await this.bridge.request('managed.protocol.cancel'))===true;}
    catch(error){this.update({error:message(error)});return false;}
  }
  dispose(){this.disposed=true;this.sequence++;this.listeners.clear();}
}
