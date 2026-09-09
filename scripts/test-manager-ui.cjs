// Execute the actual UI script with a minimal DOM adapter; no browser dependency.
const {readFileSync} = require('node:fs');
const {runInNewContext} = require('node:vm');
const assert = require('node:assert/strict');
const source = readFileSync('noc-manager/src/assets/manager.js','utf8');
async function render(page) {
  const nodes = new Map();
  const node = selector => {
    if (selector === 'input[name=rdp_use_local]' || selector === '#rdp-server-select') return null;
    if (!nodes.has(selector)) nodes.set(selector, {innerHTML:'',textContent:'',dataset:{page},value:'',
      addEventListener(){},querySelectorAll(){return [];},contains(){return false;},classList:{toggle(){},remove(){},add(){}}});
    return nodes.get(selector);
  };
  const heartbeat={state:'STARTING',last_seen:1000,version:'test',build:'test'};
  const status={now:1000,stale_after_seconds:90,retention_days:30,warning:null,
    kiosks:[{username:'linux-configured',display_username:'display-configured',name:'Configured',enabled:true,status:'attention',agent:null,display:heartbeat}],
    clients:[{app:'display',username:'new-screen',kiosk:null,heartbeat},
      // An orphaned historical Agent must remain visible in supervision only.
      {app:'agent',username:'orphan-agent',kiosk:null,heartbeat}]};
  runInNewContext(source, {document:{querySelector:node,querySelectorAll:()=>[],activeElement:null},
    location:{search:'',pathname:page==='home'?'/':'/supervision'},history:{replaceState(){}},
    URLSearchParams,Date,AbortController,setTimeout:()=>0,clearTimeout(){},
    FormData:class { *[Symbol.iterator](){} },
    fetch:async url=>({ok:true,json:async()=>url.startsWith('/ui/status')?status:{events:[],next:null}})});
  await new Promise(resolve=>setImmediate(resolve));
  return nodes;
}
async function testInlineServer() {
  const nodes=new Map();
  const node=selector=>{
    if(selector==='#dashboard'||selector==='input[name=rdp_use_local]')return null;
    if(!nodes.has(selector))nodes.set(selector,{value:'',checked:false,events:{},children:[],innerHTML:'',textContent:'',
      addEventListener(event,fn){this.events[event]=fn;},appendChild(child){this.children.push(child);},
      querySelectorAll(){return [];},classList:{remove(){},add(){}}});
    return nodes.get(selector);
  };
  node('input[name=rdp_enabled]').checked=true;
  node('input[name=username]').value='linux-manual';
  node('input[name=url]').value='https://dashboard.example';
  node('#inline-server-name').value='Linux principal';
  node('#inline-server-address').value='10.0.0.5';
  node('#inline-server-port').value='3389';
  let submitted;
  runInNewContext(source,{document:{querySelector:node,querySelectorAll:()=>[],createElement:()=>({})},
    location:{search:'',pathname:'/kiosk/new'},URLSearchParams,Date,AbortController,setTimeout:()=>0,clearTimeout(){},
    fetch:async(url,options)=>{
      if(url==='/ui/rdp-servers'){
        submitted=options.body;
        return {ok:true,json:async()=>({id:'rdp-1',name:'Linux principal',address:'10.0.0.5',port:3389})};
      }
      return {ok:true,json:async()=>({kiosks:[],clients:[],now:1000,warning:null})};
    }});
  await nodes.get('#inline-server-save').events.click();
  assert.equal(submitted.get('name'),'Linux principal');
  assert.equal(submitted.get('address'),'10.0.0.5');
  assert.equal(submitted.has('username'),false);
  assert.equal(nodes.get('#rdp-server-select').value,'rdp-1');
  assert.equal(nodes.get('#rdp-server-select').children[0].textContent,'Linux principal — 10.0.0.5:3389');
  assert.equal(nodes.get('input[name=username]').value,'linux-manual');
  assert.equal(nodes.get('input[name=url]').value,'https://dashboard.example');
  assert.equal(nodes.get('#inline-server').open,false);
}
(async()=>{
  const home=await render('home');
  const html=home.get('#results').innerHTML, summary=home.get('#summary').innerHTML;
  assert.match(summary, /<strong>1<\/strong><span>Nouveaux écrans/);
  assert.match(html, /href="\/kiosk\/new\?display=new-screen">new-screen<\/a>/);
  assert.match(html, />Nouveau<\/span>/);
  assert.match(html, /href="\/kiosk\/new\?display=new-screen">Configurer<\/a>/);
  assert.doesNotMatch(html, /orphan-agent/);
  assert.match(html, /En attente de démarrage/);
  assert.match(html, /Démarre après la connexion RDP/);
  const supervision=await render('supervision');
  assert.match(supervision.get('#results').innerHTML, /orphan-agent/);
  assert.doesNotMatch(supervision.get('#results').innerHTML, /kiosk\/new\?display=orphan-agent/);
  assert.doesNotMatch(supervision.get('#results').innerHTML, /associate-kiosk/);
  await testInlineServer();
  console.log('PASS: display-only discovery, direct creation, Agent startup, inline RDP server creation/selection preserves manually entered fields.');
})().catch(e=>{console.error(e);process.exitCode=1;});
