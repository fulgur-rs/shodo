/* Fixed input recorder; source endpoints are Unicode scalar boundaries. */
'use strict';
(async () => {
  const output = value => {
    const pre = document.createElement('pre'); pre.id = 'result';
    pre.textContent = JSON.stringify(value); document.body.replaceChildren(pre);
  };
  try {
    const config = window.shodoCapture;
    const loaded = [];
    for (const font of config.fonts) {
      const bytes = await (await fetch(`/fonts/${font.id}`)).arrayBuffer();
      const digest = [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))]
        .map(x => x.toString(16).padStart(2, '0')).join('');
      if (digest !== font.sha256) throw Error(`font hash mismatch: ${font.id}`);
      const face = await new FontFace(font.family, bytes).load();
      document.fonts.add(face); loaded.push({id: font.id, sha256: digest, face_index: font.face_index});
    }
    await document.fonts.ready;
    const records = [];
    for (const c of config.inputs.cases) {
      const box = document.createElement('div'); box.lang = c.lang;
      Object.assign(box.style, {position: 'absolute', left: '0', top: '0', margin: '0',
        padding: '0', border: '0', fontSize: `${c.font_size}px`, lineHeight: '40px',
        fontFamily: c.font_ids.map(id => JSON.stringify(config.fonts.find(f => f.id === id).family)).join(','),
        direction: c.direction, whiteSpace: c.white_space, lineBreak: 'normal', wordBreak: 'normal',
        overflowWrap: 'normal', hyphens: 'none', tabSize: '8', fontKerning: 'auto',
        fontVariantLigatures: 'normal', fontSynthesis: 'none', textAlign: 'start'});
      document.body.appendChild(box);
      const stack = [box]; const items = []; let source16 = 0;
      for (const part of c.parts) {
        while (stack.length - 1 > part.depth) stack.pop();
        while (stack.length - 1 < part.depth) {
          const span = document.createElement('span'); stack.at(-1).appendChild(span); stack.push(span);
        }
        let parent = stack.at(-1);
        if (part.color) { const span = document.createElement('span'); span.style.color=part.color; parent.appendChild(span); parent=span; }
        if (part.atomic_width != null) {
          const atom = document.createElement('span');
          Object.assign(atom.style, {display:'inline-block', width:`${part.atomic_width}px`,
            height:`${part.atomic_height}px`, margin:'0', padding:'0', border:'0', verticalAlign:'baseline'});
          parent.appendChild(atom); items.push({element:atom, ch:'\ufffc', source16}); source16++;
        } else {
          const node = document.createTextNode(part.text); parent.appendChild(node);
          let local = 0;
          for (const ch of part.text) {
            items.push({node, local, ch, source16}); local += ch.length; source16 += ch.length;
          }
        }
      }
      const text = c.parts.map(p=>p.text).join('');
      const toByte = units => new TextEncoder().encode(text.slice(0, units)).length;
      const range = document.createRange();
      const rect = item => {
        if (item.element) return item.element.getBoundingClientRect();
        range.setStart(item.node,item.local); range.setEnd(item.node,item.local+item.ch.length);
        return range.getBoundingClientRect();
      };
      const measure = width => {
        box.style.width = `${width/64}px`;
        const visible = items.filter(i => !/^[ \t\n\r]$/.test(i.ch));
        if (!visible.length) throw Error(`no visible source: ${c.id}`);
        const first = rect(visible[0]);
        let end = source16;
        // Scan logical source order. RTL glyph x and combining/ligature overlap do not affect y.
        for (const item of items) {
          if (/^[\n\r]$/.test(item.ch)) continue;
          if (c.white_space === 'normal' && /^[ \t]$/.test(item.ch)) continue;
          const r = rect(item);
          if (r.height > 0 && r.top > first.top + 20) { end = item.source16; break; }
        }
        const sample = {width_subpixels:width, end_utf16:end, end_utf8:toByte(end)};
        const atom = items.find(i=>i.element);
        if (atom) {
          const r = rect(atom); sample.atomic={top:r.top-box.getBoundingClientRect().top,
            bottom:r.bottom-box.getBoundingClientRect().top, width:r.width, height:r.height};
        }
        return sample;
      };
      const initial = measure(c.width_subpixels);
      let boundary = null;
      const min = measure(1);
      if (min.end_utf16 < initial.end_utf16) {
        let lo=1, hi=c.width_subpixels;
        while (hi-lo > 1) {
          const mid=Math.floor((lo+hi)/2);
          if (measure(mid).end_utf16 >= initial.end_utf16) hi=mid; else lo=mid;
        }
        boundary=hi;
      }
      const widths = new Set([1,c.width_subpixels]);
      if (boundary != null) {
        for (const delta of [-64,-2,-1,0,1,2,64]) if (boundary+delta>0) widths.add(boundary+delta);
      }
      const samples=[...widths].sort((a,b)=>a-b).map(measure);
      if (boundary != null && !(measure(boundary-1).end_utf16 < initial.end_utf16 && measure(boundary).end_utf16 >= initial.end_utf16))
        throw Error(`invalid boundary: ${c.id}`);
      records.push({id:c.id, seed:c.seed, text, initial, boundary_subpixels:boundary, samples});
      box.remove();
    }
    output({format_version:1, user_agent:navigator.userAgent, fonts:loaded, records});
  } catch (error) { output({error:String(error), stack:error.stack}); }
})();
