/* User-clicked read-only diagnosis. Never reads query input values, cookies, storage,
 * full HTML, or arbitrary URLs, and never transmits diagnostics to a server. */
function inspectWorkPage() {
  const u=new URL(location.href);
  const wos=["https://www.webofscience.com","https://webofscience.clarivate.cn"].includes(u.origin) && u.pathname.startsWith("/wos/");
  const admin=["http:","https:"].includes(u.protocol) && u.hostname==="admin.ir.lib.sjtu.edu.cn";
  if((!wos && !admin) || u.username || u.password)return {error:"非工作网站"};
  const visible=el=>el.getClientRects().length>0&&getComputedStyle(el).visibility!=="hidden";
  const text=String(document.body?.innerText||"");
  const normal=s=>String(s||"").replace(/\s+/g," ").trim().slice(0,120);
  const controls=[...document.querySelectorAll('[role="combobox"],select,[aria-haspopup="listbox"]')].filter(visible).slice(0,15).map(el=>({
    tag:el.tagName,role:el.getAttribute("role")||"",label:normal(el.getAttribute("aria-label")),
    // Input text can contain a user's query. Never include it in diagnostics.
    display:["INPUT","TEXTAREA"].includes(el.tagName)?"[输入内容省略]":normal(el.tagName==="SELECT"?el.selectedOptions[0]?.textContent:el.innerText)}));
  const icons='mat-icon,.mat-icon,svg,.material-icons,.material-icons-outlined,.material-symbols-outlined,.material-symbols-rounded,.material-symbols-sharp';
  const cleanText=el=>{
    if(el.matches('input[type="submit"],input[type="button"]'))return normal(el.value);
    const parts=[];
    const visit=node=>{
      if(node.nodeType===Node.TEXT_NODE){parts.push(node.nodeValue);return;}
      if(node.nodeType!==Node.ELEMENT_NODE||node.matches(icons+',script,style,[hidden],[aria-hidden="true"]'))return;
      const style=getComputedStyle(node);
      if(style.display==='none'||style.visibility==='hidden'||style.visibility==='collapse')return;
      for(const child of node.childNodes)visit(child);
    };
    visit(el);return normal(parts.join(''));
  };
  // Button text can also contain an account name or query. Return only fixed
  // UI labels; arbitrary text and aria-label values are deliberately redacted.
  const fixed=['search','检索','搜索','檢索','搜尋','search documents','search publications','检索文献','搜索文献','文献检索','檢索文獻','clear','清除'];
  const safeLabel=value=>{const label=normal(value);return fixed.includes(label.toLowerCase())?label:label?'[非检索文案省略]':'';};
  const buttonElements=[...document.querySelectorAll('button,[role="button"],input[type="submit"],input[type="button"]')].filter(visible);
  const buttons=buttonElements.slice(0,40).map(el=>({tag:el.tagName,
    display:safeLabel(cleanText(el)),aria_label:safeLabel(el.getAttribute('aria-label')),
    has_labelledby:el.hasAttribute('aria-labelledby'),icon_count:el.querySelectorAll(icons).length,
    in_navigation:!!el.closest('nav,header,footer,aside,[role="navigation"],[role="banner"]'),
    in_form:!!el.closest('form'),form_associated:!!el.form,
    disabled:!!el.disabled||el.getAttribute('aria-disabled')==='true'}));
  // Keep the read-only result rules aligned with wos-adapter.js. Both functions
  // are serialized independently by chrome.scripting; DOM regression tests
  // compare their counts. Only counts/booleans leave this reader, never targets.
  const renderedLink=el=>{
    if(el.closest('[hidden],[inert],[aria-hidden="true"]'))return false;
    const style=getComputedStyle(el);
    if(style.display==='none'||style.visibility==='hidden'||style.visibility==='collapse')return false;
    if(el.getClientRects().length)return true;
    return style.display==='contents'&&[...el.querySelectorAll('*')].some(child=>
      !child.closest('[hidden],[inert],[aria-hidden="true"]')&&visible(child));
  };
  const recordLinks=new Set(),encodedLinks=new Set(),contentsLinks=new Set(),routerLinks=new Set();
  const linkControls=document.querySelectorAll('a[href],a[routerlink],a[ng-reflect-router-link],[role="link"][routerlink],[role="link"][ng-reflect-router-link]');
  for(const anchor of linkControls){
    if(!renderedLink(anchor))continue;
    for(const attr of ['href','routerlink','ng-reflect-router-link']){
      const value=anchor.getAttribute(attr);
      if(!value)continue;
      try{
        const link=new URL(value,location.href);
        if(link.origin!==u.origin||link.username||link.password||/%(?:2f|5c)/i.test(link.pathname))continue;
        const path=decodeURIComponent(link.pathname);
        if(!/^\/wos\/woscc\/full-record\/WOS:\d{15}\/?$/.test(path))continue;
        const key=path.replace(/\/$/,'');
        recordLinks.add(key);
        if(/%3a/i.test(link.pathname))encodedLinks.add(key);
        if(!anchor.getClientRects().length)contentsLinks.add(key);
        if(attr!=='href')routerLinks.add(key);
      }catch{}
    }
  }
  const summary=/^\/wos\/woscc\/summary\//.test(u.pathname),totals=new Set();
  if(wos&&summary){
    const unit='(?:results?|records?|documents?|(?:条|個|个|篇)?\\s*(?:结果|結果|记录|紀錄|文献|文獻))';
    const number='(?:\\d{1,3}(?:[,.]\\d{3})+|\\d{1,6})';
    const exact=new RegExp('^('+number+')\\s*'+unit+'$','i');
    const add=match=>{if(match)totals.add(Number(match[1].replace(/[,.]/g,'')));};
    for(const el of [...document.querySelectorAll('h1,h2,h3,[role="heading"],[role="tab"]')].filter(renderedLink)){
      if(!el.closest('article,form,a[href*="full-record"],[hidden],[inert],[aria-hidden="true"]'))add(normal(String(el.innerText||'').normalize('NFKC')).match(exact));
    }
    if(!totals.size&&document.body){
      const unitOnly=new RegExp('^'+unit+'$','i');
      const walker=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);
      for(let node=walker.nextNode();node;node=walker.nextNode()){
        const label=normal(String(node.nodeValue||'').normalize('NFKC'));
        if(!exact.test(label)&&!unitOnly.test(label))continue;
        let el=node.parentElement;
        for(let depth=0;el&&depth<3;depth++,el=el.parentElement){
          if(el.closest('article,form,a,[role="link"],nav,aside,[hidden],[inert],[aria-hidden="true"]'))break;
          if(renderedLink(el))add(normal(String(el.innerText||'').normalize('NFKC')).match(exact));
        }
      }
    }
  }
  const noResult=/(?:no\s+(?:results?|records?|documents?)\s+(?:were\s+)?found|your\s+search\s+(?:did\s+not\s+(?:return|find)\s+any|returned\s+no)\s+results?|您的?\s*(?:检索|搜索|檢索|搜尋)\s*(?:未找到|没有找到|沒有找到|未檢索到)\s*(?:任何)?\s*(?:结果|結果)|未找到\s*(?:任何)?\s*(?:结果|結果)|没有\s*(?:检索|搜索)\s*结果|沒有\s*(?:檢索|搜尋)\s*結果)/i.test(text);
  const busy=[...document.querySelectorAll('[aria-busy="true"],[role="progressbar"],mat-spinner,mat-progress-bar,.mat-mdc-progress-spinner')]
    .some(el=>visible(el)&&!el.closest('[hidden],[inert],[aria-hidden="true"]'));
  let recordRoute=false;
  try{recordRoute=!/%(?:2f|5c)/i.test(u.pathname)&&/^\/wos\/woscc\/full-record\/WOS:\d{15}\/?$/.test(decodeURIComponent(u.pathname));}catch{}
  return {site:u.hostname,path:u.pathname,route:u.hash.split("?")[0],
    wos_error:/Oops,?\s*something went wrong!?/i.test(text),
    summary_route:summary,record_route:recordRoute,
    zero_result:noResult||(summary&&totals.size===1&&totals.has(0)&&!recordLinks.size&&!busy),busy,
    result_total:totals.size===1?[...totals][0]:null,result_total_conflict:totals.size>1,
    canonical_record_link_count:recordLinks.size,encoded_record_link_count:encodedLinks.size,
    contents_record_link_count:contentsLinks.size,router_record_link_count:routerLinks.size,
    smart_search:/Smart Search|智能检索|智能搜索/i.test(text),
    fielded_search:/Fielded Search|字段检索|字段搜索/i.test(text),
    wos_import_button:/WOS\s*数据导入\s*[（(]\s*Txt\s*[）)]/i.test(text),
    controls,buttons,visible_button_count:buttonElements.length};
}
if(typeof module!=="undefined")module.exports={inspectWorkPage};
