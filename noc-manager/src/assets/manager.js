/* Local assets only. Read models never contain an RDP password. */
(() => {
  'use strict';
  const $ = s => document.querySelector(s);
  const esc = v => String(v == null ? '' : v).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const path = v => encodeURIComponent(v);
  const qs = new URLSearchParams(location.search);
  const page = $('#dashboard')?.dataset.page;
  let live, initialized = false, historyBusy = false, historyNext = null, historyRequest = 0;
  const labels = {ready:'Opérationnel',attention:'À vérifier',disabled:'Désactivé',online:'Présent',stale:'Périmé',discovered:'À configurer'};
  const states = {STARTING:'Initialisation',CONNECTING_TO_MANAGER:'Connexion au Manager',FETCHING_CONFIG:'Configuration en cours',CONFIG_NOT_FOUND:'Configuration absente',DISABLED:'Désactivé',CHECKING_TARGET:'Vérification de la destination',TARGET_UNAVAILABLE:'Destination indisponible',STARTING_FIREFOX:'Démarrage de Firefox',WAITING_FIREFOX:'Préparation de Firefox',LOADING_PAGE:'Chargement de la page',LOADING_SLOW:'Chargement lent',RUNNING:'Affichage en cours',RESTARTING:'Redémarrage',FIREFOX_CRASHED:'Firefox arrêté',PAGE_TIMEOUT:'Délai de chargement dépassé',MANAGER_UNAVAILABLE:'Manager indisponible',FIREFOX_COMPONENT_MISSING:'Composant Firefox absent',REMOTE_RESTART_BROWSER:'Redémarrage du navigateur',REMOTE_RESTART_AGENT:'Redémarrage de l’Agent',CONNECTING:'Connexion RDP',CONNECTED:'RDP connecté',RECONNECTING:'Reconnexion RDP',ERROR:'Erreur RDP'};
  const badge = (kind, text) => `<span class="badge ${esc(kind)}">${esc(text || labels[kind] || kind)}</span>`;
  const appName = app => app === 'agent' ? 'Agent' : 'Display';
  const date = t => new Date(t * 1000).toLocaleString('fr-FR');
  const age = t => { const n = Math.max(0, (live?.now || Date.now()/1000) - t); return n < 60 ? `${Math.floor(n)} s` : n < 3600 ? `${Math.floor(n/60)} min` : n < 86400 ? `${Math.floor(n/3600)} h` : `${Math.floor(n/86400)} j`; };
  const detailUrl = (app,user) => `/supervision?app=${path(app)}&client=${path(user)}`;
  const isNewDisplay = c => c.app === 'display' && !c.kiosk;
  const configureUrl = (_app,user) => `/kiosk/new?display=${path(user)}`;
  function beatStatus(h, app) {
    if (!h) return app==='agent'?badge('pending','En attente de démarrage'):badge('disabled','Jamais vu');
    if (live.now - h.last_seen > live.stale_after_seconds) return badge('stale','Communication périmée');
    const kind = ['RUNNING','CONNECTED'].includes(h.state) ? 'ready' : /ERROR|CRASHED|UNAVAILABLE|TIMEOUT|MISSING|NOT_FOUND/.test(h.state) ? 'error' : 'pending';
    return badge(kind, states[h.state] || h.state || 'État non renseigné');
  }
  async function json(url) {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 8000);
    try { const r = await fetch(url, {cache:'no-store',signal:controller.signal}); if (!r.ok) throw new Error(`HTTP ${r.status}`); return await r.json(); }
    finally { clearTimeout(timeout); }
  }
  function summary() {
    const k = live.kiosks;
    const stats = [ ['','Configurés',k.length,'/kiosks'], ['ready','Opérationnels',k.filter(x=>x.status==='ready').length,'/kiosks?filter=ready'], ['attention','À vérifier',k.filter(x=>x.status==='attention').length,'/kiosks?filter=attention'], ['disabled','Désactivés',k.filter(x=>!x.enabled).length,'/kiosks?filter=disabled'], ['discovered','Nouveaux écrans',live.clients.filter(isNewDisplay).length,'/kiosks?filter=discovered'] ];
    $('#summary').innerHTML = stats.map(([kind,label,n,url])=>`<a class="stat ${kind}" href="${url}"><strong>${n}</strong><span>${label}</span></a>`).join('');
    $('#storage-warning').innerHTML = live.warning ? `<p class="error">${esc(live.warning)}</p>` : '';
    ['agent','display'].forEach(app => { const list = $(`#${app}-names`); if(list) list.innerHTML=live.clients.filter(c=>c.app===app).map(c=>`<option value="${esc(c.username)}">${c.kiosk?'Associé à '+esc(c.kiosk):(app==='display'?'Nouveau':'Non associé')}</option>`).join(''); });
  }
  const empty = text => `<p class="empty">${text}</p>`;
  function init() {
    document.querySelectorAll('nav a').forEach(a=>a.classList.toggle('active', a.getAttribute('href') === (location.pathname.startsWith('/rdp-servers') ? '/rdp-servers' : page === 'home' ? '/' : page === 'supervision' ? '/supervision' : '/kiosks')));
    if (!page) return;
    const filterOptions = page === 'supervision' ? [['','Tous les clients'],['discovered','Nouveaux écrans'],['online','Présents'],['stale','Communication périmée']] : [['','Tous les kiosques'],['ready','Opérationnels'],['attention','À vérifier'],['disabled','Désactivés'],['discovered','Nouveaux écrans']];
    $('#workspace').innerHTML = `<p class="subtitle">${page==='home'?'Une vue d’ensemble de vos écrans et de leurs connexions.':page==='kiosks'?'Configurez vos kiosques et associez les sessions Agent et Display.':'Tous les clients détectés et chaque heartbeat reçu, même sans configuration.'}</p>
      <div id="flash"></div>
      <form class="toolbar" id="filters"><label class="search">Rechercher<input name="q" placeholder="Nom du kiosque ou de la session" value="${esc(qs.get('q')||'')}"></label>
      <label>Statut<select name="filter">${filterOptions.map(([v,t])=>`<option value="${v}" ${qs.get('filter')===v?'selected':''}>${t}</option>`).join('')}</select></label>
      ${page==='supervision'?`<label>Application<select name="app"><option value="">Agent et Display</option><option value="agent">Agent</option><option value="display">Display</option></select></label>`:''}
      <button class="btn" type="submit">Filtrer</button><a class="btn" href="${location.pathname}">Réinitialiser</a></form>
      <div id="client-detail"></div><div id="results"></div>${page==='supervision'?'<section id="history-panel"></section>':''}`;
    $('#filters').addEventListener('submit', e=>{e.preventDefault();updateFilters();});
    $('#filters [name=q]').addEventListener('input',updateFilters);
    $('#filters [name=filter]').addEventListener('change',updateFilters);
    if(page==='supervision') { $('#filters [name=app]').value=qs.get('app')||''; $('#filters [name=app]').addEventListener('change',updateFilters); initHistory(); }
    const sent=qs.get('sent');
    if(['restart_browser','restart_agent'].includes(sent)) $('#flash').innerHTML=`<p class="flash">Commande envoyée : ${sent==='restart_browser'?'redémarrer Firefox':'redémarrer l’Agent'}. Elle sera exécutée à la prochaine interrogation du Manager.</p>`;
  }
  function updateFilters() {
    for(const [key,value] of new FormData($('#filters'))) { if(value) qs.set(key,value);else qs.delete(key); }
    history.replaceState(null,'',location.pathname+(qs.size?'?'+qs.toString():'')); render();
  }
  function matches(text){return text.toLocaleLowerCase().includes((qs.get('q')||'').toLocaleLowerCase());}
  function component(app,user,h){return `<div class="component"><div><span class="type">${appName(app)}</span><a href="${detailUrl(app,user)}">${esc(user)}</a></div><div class="right">${beatStatus(h,app)}<small>${h?'Reçu il y a '+age(h.last_seen):app==='agent'?'Démarre après la connexion RDP':'Aucun heartbeat reçu'}</small></div></div>`;}
  function clientRows(clients){
    if(!clients.length)return empty('Aucun client ne correspond à ces filtres.');
    return `<div class="table-wrap"><table><thead><tr><th>Client / session</th><th>Application</th><th>État rapporté</th><th>Dernier heartbeat</th><th>Association</th><th></th></tr></thead><tbody>${clients.map(c=>`<tr><td><a class="row-title" href="${isNewDisplay(c)?configureUrl(c.app,c.username):detailUrl(c.app,c.username)}">${esc(c.username)}</a> ${isNewDisplay(c)?badge("discovered","Nouveau"):""}<small>v${esc(c.heartbeat.version)||'—'}</small></td><td>${appName(c.app)}</td><td>${beatStatus(c.heartbeat)}</td><td>${esc(date(c.heartbeat.last_seen))}<small>Il y a ${age(c.heartbeat.last_seen)}</small></td><td>${c.kiosk?`<a href="/kiosk/${path(c.kiosk)}/edit">${esc(c.kiosk)}</a>`:(isNewDisplay(c)?badge('discovered','Nouveau'):badge('disabled','Non associé'))}</td><td><a class="btn" href="${isNewDisplay(c)?configureUrl(c.app,c.username):detailUrl(c.app,c.username)}">${isNewDisplay(c)?'Configurer':'Détails'}</a></td></tr>`).join('')}</tbody></table></div>`;
  }
  function render(){
    if(!page)return;
    const filter=qs.get('filter')||'';
    let clients=live.clients.filter(c=>matches(c.username+' '+(c.kiosk||'')));
    const unconfigured=clients.filter(isNewDisplay);
    let kiosks=live.kiosks.filter(k=>matches(k.name+' '+k.username+' '+k.display_username)&&(!filter||k.status===filter));
    if(page==='supervision'){
      clients=clients.filter(c=>(!qs.get('app')||c.app===qs.get('app'))&&(!filter||(filter==='discovered'?isNewDisplay(c):c.presence===filter)));
      if(qs.get('client')) clients=clients.filter(c=>c.username===qs.get('client'));
      $('#results').innerHTML=`<div class="section-head"><h2>Clients détectés <span class="count">${clients.length}</span></h2></div>${clientRows(clients)}`;
      renderDetail(); return;
    }
    const discovered=(filter===''||filter==='discovered')?`<div class="section-head"><h2>Nouveaux écrans <span class="count">${unconfigured.length}</span></h2><a href="/supervision?filter=discovered">Voir la supervision →</a></div>${unconfigured.length?clientRows(unconfigured):empty('Aucun nouvel écran à configurer.')}`:'';
    if(filter==='discovered'){$('#results').innerHTML=discovered;return;}
    let content;
    if(!kiosks.length)content=empty(live.kiosks.length?'Aucun kiosque ne correspond à ces filtres.':'Aucun kiosque configuré. Ajoutez votre premier kiosque ou configurez un nouvel écran détecté.');
    else if(page==='home')content=`<div class="grid">${kiosks.map(k=>`<article class="kiosk-card"><div class="card-top"><div><h3>${esc(k.name)}</h3><p class="muted">${esc(k.username)}</p></div>${badge(k.status)}</div>${component('agent',k.username,k.agent)}${component('display',k.display_username,k.display)}${k.pending?`<p class="pending-note">Commande en attente : ${esc(k.pending)}</p>`:''}<div class="card-bottom"><span class="muted">${k.restart_cron?'Redémarrage planifié':'Sans redémarrage planifié'}</span><a href="/kiosks?q=${path(k.username)}">Gérer le kiosque →</a></div></article>`).join('')}</div>`;
    else content=`<div class="table-wrap"><table><thead><tr><th>Kiosque</th><th>Statut</th><th>Agent</th><th>Display</th><th>Planification / commande</th><th>Actions</th></tr></thead><tbody>${kiosks.map(k=>`<tr><td><a class="row-title" href="/kiosk/${path(k.username)}/edit">${esc(k.name)}</a><small>${esc(k.username)}</small></td><td>${badge(k.status)}</td><td><a href="${detailUrl('agent',k.username)}">${esc(k.username)}</a><small>${beatStatus(k.agent,'agent')}</small></td><td><a href="${detailUrl('display',k.display_username)}">${esc(k.display_username)}</a><small>${beatStatus(k.display)}</small></td><td><code>${esc(k.restart_cron)||'—'}</code><small>${esc(k.pending)||'Aucune commande en attente'}</small></td><td><div class="actions"><a class="btn" href="/kiosk/${path(k.username)}/edit">Modifier</a>${command(k,'restart-browser','Firefox')}${command(k,'restart-agent','Agent')}${command(k,'delete','Supprimer')}</div></td></tr>`).join('')}</tbody></table></div>`;
    $('#results').innerHTML=`<div class="section-head"><h2>${page==='home'?'Vos kiosques':'Kiosques configurés'} <span class="count">${kiosks.length}</span></h2><span class="muted">Agent actif + RDP connecté = opérationnel</span></div>${content}${discovered}`;
    $('#results').querySelectorAll('form[data-confirm]').forEach(f=>f.addEventListener('submit',e=>{if(!confirm(f.dataset.confirm))e.preventDefault();}));
  }
  function command(k,action,label){const msg=action==='delete'?`Supprimer la configuration de « ${k.name} » ? Les clients et leur historique seront conservés.`:`Envoyer une commande de redémarrage ${label} à « ${k.name} » ?`;return `<form method="post" action="/kiosk/${path(k.username)}/${action}" data-confirm="${esc(msg)}"><button class="btn ${action==='delete'?'danger':''}" type="submit">${action==='delete'?'':'↻ '}${label}</button></form>`;}
  function renderDetail(){
    const user=qs.get('client'),app=qs.get('app');
    if(!user||!app){$('#client-detail').innerHTML='';return;}
    const c=live.clients.find(c=>c.app===app&&c.username===user),h=c?.heartbeat;
    const linked=c?.kiosk||live.kiosks.find(k=>(app==='agent'?k.username:k.display_username)===user)?.username;
    $('#client-detail').innerHTML=`<section><div class="section-head"><h2>${appName(app)} · ${esc(user)}</h2>${linked?`<a class="btn" href="/kiosk/${path(linked)}/edit">Modifier le kiosque</a>`:h&&app==='display'?`<a class="btn primary" href="${configureUrl(app,user)}">Configurer</a>`:''}</div><div class="detail-grid"><div class="detail-item"><small>État actuel</small>${beatStatus(h,app)}</div><div class="detail-item"><small>Dernière réception</small>${h?esc(date(h.last_seen)):'Jamais vu'}</div><div class="detail-item"><small>Version / build</small>${h?`v${esc(h.version)||'—'}<br><code>${esc(h.build)||'—'}</code>`:'—'}</div><div class="detail-item"><small>Association</small>${esc(linked)||'Aucune configuration détectée'}</div></div></section>`;
  }
  function localInput(epoch){if(!epoch)return '';const d=new Date(Number(epoch)*1000);if(!Number.isFinite(d.getTime()))return '';d.setMinutes(d.getMinutes()-d.getTimezoneOffset());return d.toISOString().slice(0,16);}
  function initHistory(){
    $('#history-panel').innerHTML=`<div class="section-head"><h2>Historique des heartbeats</h2><span class="muted">Conservation : ${live.retention_days} jours</span></div><form class="toolbar" id="history-filters"><label>Application<select name="app"><option value="">Toutes</option><option value="agent">Agent</option><option value="display">Display</option></select></label><label class="search">Session exacte<input name="username" value="${esc(qs.get('client')||qs.get('history_user')||'')}" placeholder="Toutes les sessions"></label><label>État<select name="state"><option value="">Tous les états</option>${Object.keys(states).map(s=>`<option value="${s}">${esc(states[s])} (${s})</option>`).join('')}</select></label><label>Du<input type="datetime-local" name="from" value="${localInput(qs.get('from'))}"></label><label>Au<input type="datetime-local" name="to" value="${localInput(qs.get('to'))}"></label><button class="btn" type="submit">Rechercher</button></form><div id="history-error" role="alert"></div><div id="history-results"></div><div class="pagination"><span id="history-count"></span><div class="actions"><button id="history-first" class="btn">Plus récents</button><button id="history-next" class="btn" disabled>Plus anciens →</button></div></div>`;
    $('#history-filters [name=app]').value=qs.get('history_app')||qs.get('app')||'';
    $('#history-filters [name=state]').value=qs.get('state')||'';
    $('#history-filters').addEventListener('submit',e=>{e.preventDefault();qs.delete('before');saveHistoryFilters();loadHistory();});
    $('#history-first').addEventListener('click',()=>{qs.delete('before');saveHistoryFilters();loadHistory();});
    $('#history-next').addEventListener('click',()=>{if(historyNext){qs.set('before',historyNext);saveHistoryFilters();loadHistory();}});
    loadHistory();
  }
  function historyQuery(){const q=new URLSearchParams();for(const [key,value] of new FormData($('#history-filters'))){if(value){const v=['from','to'].includes(key)?Math.floor(new Date(value).getTime()/1000):value;q.set(key,v);}}if(qs.get('before'))q.set('before',qs.get('before'));return q;}
  function saveHistoryFilters(){const q=historyQuery();for(const [formKey,urlKey] of [['app','history_app'],['username','history_user'],['state','state'],['from','from'],['to','to']]){if(q.has(formKey))qs.set(urlKey,q.get(formKey));else qs.delete(urlKey);}history.replaceState(null,'',location.pathname+'?'+qs.toString());}
  async function loadHistory(){
    const request=++historyRequest;historyBusy=true;$('#history-error').innerHTML='';$('#history-next').disabled=true;
    try{const q=historyQuery();if(q.has('from')&&q.has('to')&&Number(q.get('from'))>Number(q.get('to')))throw new Error('La date de début doit précéder la date de fin.');
      const data=await json('/ui/history?'+q);if(request!==historyRequest)return;historyNext=data.next;
      $('#history-results').innerHTML=data.events.length?`<div class="table-wrap"><table><thead><tr><th>Réception</th><th>Client</th><th>Application</th><th>État envoyé</th><th>Version</th><th>Build</th></tr></thead><tbody>${data.events.map(e=>`<tr><td>${esc(date(e.last_seen))}<small>#${e.id}</small></td><td><a href="${detailUrl(e.app,e.username)}">${esc(e.username)}</a></td><td>${appName(e.app)}</td><td class="history-state">${esc(states[e.state]||e.state||'Non renseigné')}<code>${esc(e.state)}</code></td><td>${esc(e.version)||'—'}</td><td><code>${esc(e.build)||'—'}</code></td></tr>`).join('')}</tbody></table></div>`:empty('Aucun heartbeat reçu sur cette période avec ces filtres.');
      $('#history-count').textContent=`${data.events.length} événement(s) affiché(s) · 100 par page`;
      $('#history-next').disabled=!historyNext;
    }catch(e){if(request===historyRequest)$('#history-error').innerHTML=`<p class="error">Impossible d’actualiser l’historique. ${esc(e.message)} Les données précédentes peuvent être périmées.</p>`;}
    finally{if(request===historyRequest)historyBusy=false;}
  }
  async function refresh(){
    try{live=await json('/ui/status');summary();if(!initialized){init();initialized=true;}render();
      $('#sync-status').textContent=`Mis à jour à ${new Date().toLocaleTimeString('fr-FR')} · actualisation toutes les 10 s`;
      $('.sync-line').classList.remove('failed');
      if(page==='supervision'&&!historyBusy&&!qs.get('before')&&initialized&&!$('#history-filters').contains(document.activeElement))loadHistory();
    }catch(e){$('#sync-status').textContent='Actualisation indisponible · les statuts affichés peuvent être périmés';$('.sync-line').classList.add('failed');}
    finally{setTimeout(refresh,10000);}
  }
  const localChoice = $('input[name=rdp_use_local]');
  if (localChoice) {
    const password = $('input[name=rdp_password]'), rdp = $('input[name=rdp_enabled]');
    const updatePassword = () => { password.required = rdp.checked && !localChoice.checked; password.disabled = localChoice.checked; };
    localChoice.addEventListener('change', updatePassword); rdp.addEventListener('change', updatePassword); updatePassword();
  }
  const serverSelect = $('#rdp-server-select');
  if (serverSelect) {
    const rdp = $('input[name=rdp_enabled]');
    const updateServer = () => {
      const manual = serverSelect.value === '__manual';
      $('#rdp-manual-fields').hidden = !manual;
      serverSelect.required = rdp.checked;
      $('#rdp-manual-fields').querySelectorAll('input').forEach(input => { input.disabled = !manual; });
    };
    serverSelect.addEventListener('change', updateServer); rdp.addEventListener('change', updateServer); updateServer();
    $('#inline-server-save').addEventListener('click', async () => {
      const button = $('#inline-server-save'), message = $('#inline-server-status');
      button.disabled = true; message.textContent = 'Enregistrement…';
      const body = new URLSearchParams({name:$('#inline-server-name').value,address:$('#inline-server-address').value,port:$('#inline-server-port').value});
      if ($('#inline-server-certificate').checked) body.set('ignore_certificate_errors','1');
      try {
        const response = await fetch('/ui/rdp-servers', {method:'POST',body});
        const server = await response.json();
        if (!response.ok) throw new Error(server.error || 'Enregistrement impossible');
        const option = document.createElement('option');
        option.value = server.id; option.textContent = `${server.name} — ${server.address}:${server.port}`;
        serverSelect.appendChild(option); serverSelect.value = server.id; updateServer();
        message.textContent = 'Serveur enregistré et sélectionné. Vous pouvez terminer le kiosque.';
        $('#inline-server').open = false;
      } catch (e) { message.textContent = e.message; }
      finally { button.disabled = false; }
    });
  }
  refresh();
})();
