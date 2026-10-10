/* Native loopback-only import receipts; the production adapter is unchanged. */
const nativeSubmit=drawer.onSubmit.bind(drawer);
drawer.onSubmit=function(){
 const payload={form:JSON.parse(JSON.stringify(this.form)),files:this.fileList.map(f=>({name:f.name,size:f.size,response:f.response}))};
 fetch('/native/import-submit',{method:'POST',body:JSON.stringify(payload)}).then(r=>{if(r.ok)nativeSubmit();});
};
const nativePush=push.onSubmit.bind(push);
push.onSubmit=function(){
 fetch('/native/import-push',{method:'POST',body:JSON.stringify(this.form)}).then(r=>{if(r.ok)nativePush();});
};
drawer.$el.querySelector('input').addEventListener('change',async e=>{
 const file=e.target.files[0];
 await fetch('/native/import-upload',{method:'POST',body:JSON.stringify({name:file.name,size:file.size,text:await file.text()})});
});
