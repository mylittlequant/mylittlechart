/* ============================================================
   mlc sim — runtime.

   No chart, no bars, no data. The chart pane is empty on purpose;
   objects are spawned from the sim's own control plane and reach
   their settings THE SAME WAY they do in the app: Object Tree row
   → gear. That path is part of what the sim is testing.

   Agent control plane: window.__SIM (see the bottom of this file).
   Everything an agent needs is callable from CDP eval; the page
   never opens a socket and the dev server stays a dumb static one.
   ============================================================ */

(function () {
  'use strict';

  const { TYPES, APP_MODALS, templatesFor } = window.SIM_REGISTRY;
  const ICONS = window.SIM_ICONS || {};
  const $ = (sel, root) => (root || document).querySelector(sel);
  const el = (tag, cls, html) => {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    if (html != null) n.innerHTML = html;
    return n;
  };

  const state = {
    objects: [],      // { id, typeId, name }
    selected: null,   // object id
    modal: null,      // { kind:'type', objId, tab } | { kind:'app', id, tab }
    seq: 0,
    tmplOpen: false,
    current: {},      // objId -> applied template name
  };

  const typeOf = (id) => TYPES.find((t) => t.id === id);
  const appOf = (id) => APP_MODALS.find((m) => m.id === id);
  const objOf = (id) => state.objects.find((o) => o.id === id);

  // ---- icons -----------------------------------------------------------
  // The app's own SVGs, exported from mlc-core/src/state/icons.rs. `currentColor`
  // means the icon takes the button's colour, exactly as the Rust renderer does.

  function iconNode(name, size) {
    const svg = ICONS[name];
    const n = el('span', 'ico');
    if (size) n.style.setProperty('--ico', size + 'px');
    n.innerHTML = svg || '';
    if (!svg) n.textContent = '·';
    return n;
  }

  function paintIcons(root) {
    (root || document).querySelectorAll('[data-icon]').forEach((n) => {
      if (n.dataset.painted) return;
      n.dataset.painted = '1';
      n.insertBefore(iconNode(n.dataset.icon), n.firstChild);
    });
  }

  // ---- spawner ---------------------------------------------------------

  const sel = $('#spawn-type');
  TYPES.forEach((t) => {
    const o = document.createElement('option');
    o.value = t.id;
    o.textContent = t.name;
    sel.appendChild(o);
  });

  function spawn(typeId) {
    const t = typeOf(typeId) || TYPES[0];
    const n = state.objects.filter((o) => o.typeId === t.id).length + 1;
    const obj = { id: 'obj' + ++state.seq, typeId: t.id, name: t.name + (n > 1 ? ' ' + n : '') };
    state.objects.push(obj);
    state.selected = obj.id;
    openSide(true);
    render();
    return obj.id;
  }

  $('#spawn-btn').onclick = () => spawn(sel.value);

  // ---- object tree ------------------------------------------------------

  function openSide(on) {
    $('#side').classList.toggle('open', on !== false);
    document.querySelectorAll('#right .rail-btn').forEach((b) => b.classList.toggle('on', b.dataset.panel === 'objects' && on !== false));
  }
  $('#side-close').onclick = () => openSide(false);
  document.querySelectorAll('#right .rail-btn').forEach((b) => {
    b.onclick = () => openSide(b.dataset.panel === 'objects' ? !$('#side').classList.contains('open') : false);
  });

  function renderTree() {
    const host = $('#tree');
    host.innerHTML = '';
    state.objects.forEach((o) => {
      const row = el('div', 'tree-row' + (o.id === state.selected ? ' sel' : ''));
      row.dataset.obj = o.id;
      const chip = iconNode(o.typeId, 14);
      chip.className = 'ico chip';
      row.appendChild(chip);
      row.appendChild(el('div', 'name', o.name));
      const acts = el('div', 'acts');
      [['lock', 'unlock'], ['eye', 'eye'], ['alert', 'alert'], ['gear', 'settings'], ['del', 'close']].forEach(([act, ico]) => {
        const s = iconNode(ico, 14);
        s.dataset.act = act;
        s.querySelectorAll('svg').forEach((v) => (v.style.pointerEvents = 'none'));
        acts.appendChild(s);
      });
      row.appendChild(acts);
      row.onclick = (e) => {
        const act = e.target.dataset && e.target.dataset.act;
        state.selected = o.id;
        if (act === 'gear') openModal(o.id);
        else if (act === 'del') {
          state.objects = state.objects.filter((x) => x.id !== o.id);
          if (state.selected === o.id) state.selected = null;
        }
        render();
      };
      host.appendChild(row);
    });
  }

  function renderObjects() {
    const host = $('#objects');
    host.innerHTML = '';
    state.objects.forEach((o, i) => {
      const box = el('div', 'obj' + (o.id === state.selected ? ' sel' : ''), o.name);
      box.style.left = 40 + (i % 4) * 150 + 'px';
      box.style.top = 40 + Math.floor(i / 4) * 46 + 'px';
      box.onclick = () => { state.selected = o.id; render(); };
      host.appendChild(box);
    });
    $('#spawner').style.display = state.objects.length ? 'none' : 'block';
  }

  // ---- modal ------------------------------------------------------------

  function openModal(objId, tabId) {
    const o = objOf(objId);
    if (!o) return;
    const spec = typeOf(o.typeId).spec;
    state.modal = { kind: 'type', objId, tab: tabId || spec.tabs[0].id };
    syncUrl();
    render();
  }

  /** Toolbar-owned modals: same shell and tokens, authored tab strip. */
  function openApp(id, tabId) {
    const m = appOf(id);
    if (!m) return;
    state.modal = { kind: 'app', id, tab: tabId || m.tabs[0].id };
    syncUrl();
    render();
  }

  function closeModal() {
    state.modal = null;
    state.tmplOpen = false;
    syncUrl();
    render();
  }

  function renderModal() {
    const bd = $('#backdrop');
    bd.classList.toggle('open', !!state.modal);
    bd.innerHTML = '';
    if (!state.modal) return;

    const isType = state.modal.kind === 'type';
    const o = isType ? objOf(state.modal.objId) : null;
    const type = isType ? typeOf(o.typeId) : null;
    const app = isType ? null : appOf(state.modal.id);
    const spec = isType ? type.spec : app;
    const title = isType ? o.name : app.name;
    const tab = spec.tabs.find((t) => t.id === state.modal.tab) || spec.tabs[0];
    bd.appendChild(buildModal({ isType, spec, title, tab, type, obj: o, app, live: true }));
    bd.onclick = (e) => { if (e.target === bd) closeModal(); };
  }

  /**
   * One modal node. Shared by the live modal and the gallery so the contact
   * sheet cannot drift from what actually opens — a gallery drawn by its own
   * code would be a second implementation to keep honest.
   */
  function buildModal({ isType, spec, title, tab, type, obj, app, live }) {
    const w = isType ? 560 : app.w;      // sim default; the app remembers a per-modal size
    const h = isType ? 620 : app.h;
    const m = el('div', 'modal');
    m.style.width = w + 'px';
    m.style.height = h + 'px';
    m.style.left = `calc(50% - ${w / 2}px)`;
    m.style.top = `calc(50% - ${h / 2}px)`;

    const head = el('div', 'head', `<span>${title}</span>`);
    const x = el('span', 'x', '✕');
    x.onclick = closeModal;
    head.appendChild(x);
    m.appendChild(head);
    dragBy(head, m);

    const tabs = el('div', 'tabs');
    spec.tabs.forEach((t) => {
      const b = el('div', 'tab' + (t.id === tab.id ? ' on' : ''), t.label);
      b.dataset.tab = t.id;
      b.onclick = () => { state.modal.tab = t.id; syncUrl(); render(); };
      tabs.appendChild(b);
    });
    m.appendChild(tabs);

    const body = el('div', 'body');
    tab.groups.forEach((g) => body.appendChild(renderGroup(g)));
    m.appendChild(body);

    const foot = el('div', 'foot');
    if (isType) {
      const tmpl = el('div', 'tmpl', 'Шаблоны');
      tmpl.onclick = (e) => { e.stopPropagation(); toggleTemplates(type, obj, tmpl); };
      foot.appendChild(tmpl);
    }
    const search = el('input', 'search');
    search.type = 'text';
    search.placeholder = 'поиск настройки…';
    foot.appendChild(search);
    const ok = el('div', 'ok', 'OK');
    ok.onclick = closeModal;
    foot.appendChild(ok);
    m.appendChild(foot);

    m.appendChild(el('div', 'grip'));
    return m;
  }

  function renderGroup(g) {
    const box = el('div', 'group');
    const gh = el('div', 'gh', `<span>${g.title}</span>` + (g.hint ? `<em>${g.hint}</em>` : ''));
    box.appendChild(gh);
    const gb = el('div', 'gb');
    g.rows.forEach((r) => gb.appendChild(renderRow(r)));
    box.appendChild(gb);
    return box;
  }

  const CTL = {
    color: (r) => `<div class="sw" style="background:${r.v};${r.dim ? 'opacity:.4' : ''}"></div>`,
    num: (r) => `<input class="num" value="${r.v}">`,
    dd: (r) => `<div class="dd"><span>${r.v}</span><span class="chev">▾</span></div>`,
    slider: (r) => `<div class="slider"><b style="width:${r.pct}%"></b><i style="left:${r.pct}%"></i></div><span class="note">${r.v}</span>`,
  };

  function renderRow(r) {
    switch (r.k) {
      case 'note': return el('div', 'row note', r.text);
      case 'hr': return el('div', 'hr');

      case 'chk': {
        const row = el('div', 'row', `<span class="lab">${r.lab}</span>`);
        const ctl = el('div', 'ctl');
        ctl.appendChild(checkbox(r.on));
        row.appendChild(ctl);
        return row;
      }

      case 'chk3': {
        const row = el('div', 'row');
        r.labs.forEach((l, i) => {
          const wrap = el('div', 'ctl');
          wrap.style.marginLeft = i ? 'auto' : '0';
          wrap.appendChild(checkbox(r.on[i]));
          wrap.appendChild(el('span', 'lab', l));
          row.appendChild(wrap);
        });
        return row;
      }

      case 'color': case 'num': case 'dd': case 'slider': {
        const row = el('div', 'row', `<span class="lab">${r.lab || ''}</span>`);
        row.appendChild(el('div', 'ctl', CTL[r.k](r)));
        return row;
      }

      case 'mode': {
        const row = el('div', 'row');
        const pills = el('div', 'pills');
        r.opts.forEach((o, i) => pills.appendChild(el('div', 'pill' + (i === r.on ? ' on' : ''), o)));
        row.appendChild(pills);
        return row;
      }

      case 'pills': {
        const row = el('div', 'row');
        if (r.lab) row.appendChild(el('span', 'lab', r.lab));
        const pills = el('div', 'pills');
        r.opts.forEach((o, i) => pills.appendChild(el('div', 'pill' + (i === r.on ? ' on' : ''), o)));
        row.appendChild(pills);
        return row;
      }

      case 'icons': {
        const wrap = el('div', 'pills');
        r.opts.forEach((o, i) => wrap.appendChild(el('div', 'pill' + (i === r.on ? ' on' : ''), o)));
        return wrap;
      }

      case 'fillmaster': {
        const row = el('div', 'row', `<span class="lab">${r.lab}</span>`);
        const ctl = el('div', 'ctl', CTL.slider(r));
        ctl.appendChild(checkbox(r.on));
        row.appendChild(ctl);
        return row;
      }

      case 'ramps': {
        const wrap = el('div', 'ramps');
        [['Classic', ['#f23645', '#ff9800', '#ffe700', '#26a69a', '#2962ff']],
         ['Warm', ['#7a1f1f', '#b34700', '#e08a00', '#ffb300', '#ffd54f']],
         ['Cool', ['#0b3d91', '#1565c0', '#1e88e5', '#26c6da', '#80deea']],
         ['Mono', ['#3a3f4b', '#575d6b', '#767c8a', '#9aa0ad', '#c3c7d1']]].forEach(([name, cols], i) => {
          const card = el('div', 'ramp' + (i === 0 ? ' on' : ''));
          const strip = el('div', 'strip');
          cols.forEach((c) => {
            const s = el('span');
            s.style.background = c;
            strip.appendChild(s);
          });
          card.appendChild(strip);
          card.appendChild(el('div', 'cap', name));
          wrap.appendChild(card);
        });
        return wrap;
      }

      case 'ladder': {
        const wrap = el('div', 'ladder');
        r.values.forEach((v, i) => {
          const row = el('div', 'lrow');
          row.appendChild(checkbox(i > 1 && i < 6));
          const val = el('input', 'val');
          val.value = fmtLevel(v, r.unit);
          row.appendChild(val);
          const bar = el('div', 'bar');
          const hue = 8 + i * 26;
          bar.style.background = `hsl(${hue} 70% 45% / .18)`;
          const line = el('i');
          line.style.background = `hsl(${hue} 70% 55%)`;
          bar.appendChild(line);
          row.appendChild(bar);
          const chip = el('div', 'chip');
          chip.style.background = `hsl(${hue} 70% 55%)`;
          row.appendChild(chip);
          row.appendChild(checkbox(false));
          row.appendChild(el('div', 'del', '✕'));
          wrap.appendChild(row);
        });
        wrap.appendChild(el('div', 'add-row', '＋ добавить уровень'));
        return wrap;
      }

      case 'table': {
        const t = el('table', 'grid');
        const thead = el('tr');
        r.head.forEach((h) => thead.appendChild(el('th', null, h)));
        t.appendChild(thead);
        r.rows.forEach((cells) => {
          const tr = el('tr');
          cells.forEach((c, i) => {
            const td = el('td', i === 0 ? 'k' : null);
            if (c && typeof c === 'object') {
              if (c.sw) td.innerHTML = `<div class="sw" style="background:${c.sw}"></div>`;
              else if ('chk' in c) td.appendChild(checkbox(c.chk));
              else td.innerHTML = `<span class="${c.cls || ''}">${c.t}</span>`;
            } else if (i === 0) td.textContent = c;
            else td.innerHTML = `<input value="${c}">`;
            tr.appendChild(td);
          });
          t.appendChild(tr);
        });
        return t;
      }

      case 'stat': {
        const row = el('div', 'stat', `<span>${r.lab}</span><b class="${r.cls || ''}">${r.v}</b>`);
        return row;
      }

      case 'area': {
        const a = el('textarea');
        a.value = r.v;
        a.style.cssText = 'width:100%;height:56px;background:var(--mlc-colors-toolbar-bg);border:1px solid var(--mlc-widget-border-normal);border-radius:var(--mlc-sizing-border-radius);color:var(--mlc-colors-text-primary);font:inherit;font-size:var(--mlc-fonts-size-small);padding:5px 7px;resize:none';
        return a;
      }

      case 'stepper': {
        const row = el('div', 'row', `<span class="lab">${r.lab}</span>`);
        const ctl = el('div', 'ctl');
        ctl.appendChild(el('div', 'pill', '−'));
        ctl.appendChild(el('div', 'num', String(r.v)));
        ctl.appendChild(el('div', 'pill', '+'));
        row.appendChild(ctl);
        return row;
      }

      case 'cells': {
        const grid = el('div');
        grid.style.cssText = `display:grid;grid-template-columns:repeat(${r.cols},1fr);gap:var(--mlc-ps-cl-cell-gap)`;
        r.rows.forEach((line, ri) => line.forEach((c) => {
          const inp = el('input');
          inp.value = c;
          inp.style.cssText = 'height:var(--mlc-ps-cl-cell-h);background:var(--mlc-colors-toolbar-bg);border:1px solid var(--mlc-widget-border-normal);border-radius:3px;color:var(--mlc-colors-text-primary);font:inherit;font-size:var(--mlc-fonts-size-small);padding:0 5px' + (ri === 0 ? ';background:var(--mlc-widget-bg-normal);font-weight:var(--mlc-fonts-weight-medium)' : '');
          grid.appendChild(inp);
        }));
        const acts = el('div', 'pills');
        acts.style.marginTop = '6px';
        ['+ строка', '+ колонка', '− строка', '− колонка'].forEach((t) => acts.appendChild(el('div', 'pill', t)));
        const wrap = el('div');
        wrap.appendChild(grid);
        wrap.appendChild(acts);
        return wrap;
      }

      case 'range': {
        const row = el('div', 'row', `<span class="lab">${r.lab}</span>`);
        const ctl = el('div', 'ctl');
        ctl.style.flex = '1';
        const s = el('div', 'slider');
        s.style.flex = '1';
        s.innerHTML = `<b style="left:${r.from}%;right:${100 - r.to}%"></b><i style="left:${r.from}%"></i><i style="left:${r.to}%"></i>`;
        ctl.appendChild(s);
        ctl.appendChild(el('span', 'note', r.cap));
        row.appendChild(ctl);
        return row;
      }

      default: return el('div', 'row note', 'неизвестная строка: ' + r.k);
    }
  }

  function checkbox(on) {
    const c = el('div', 'chk' + (on ? ' on' : ''), '✓');
    c.onclick = (e) => { e.stopPropagation(); c.classList.toggle('on'); };
    return c;
  }

  function fmtLevel(v, unit) {
    if (unit === 'gann') {
      // The ladder asks the TYPE how to write a value — a Gann ray is 2x1, not 2.000.
      if (v >= 1) return `${Math.round(v)}x1`;
      return `1x${Math.round(1 / v)}`;
    }
    return v.toFixed(3);
  }

  // ---- template menu ----------------------------------------------------

  function toggleTemplates(type, obj, anchor) {
    const menu = $('#tmplmenu');
    state.tmplOpen = !state.tmplOpen;
    menu.classList.toggle('open', state.tmplOpen);
    if (!state.tmplOpen) return;
    menu.innerHTML = '';
    const applied = state.current[obj.id];
    const names = templatesFor(type);
    // "Save as…" is offered only while the object DRIFTS from its reference —
    // right after an apply the state equals the template, so the row is absent.
    if (!applied) menu.appendChild(item('Save as…', false, () => {}));
    names.forEach((n) => menu.appendChild(item(n, applied === n, () => {
      state.current[obj.id] = n;
      state.tmplOpen = false;
      menu.classList.remove('open');
      render();
    })));
    const r = anchor.getBoundingClientRect();
    menu.style.left = r.left + 'px';
    menu.style.top = r.bottom + 4 + 'px';

    function item(label, on, fn) {
      const i = el('div', 'item' + (on ? ' on' : ''), label);
      i.onclick = (e) => { e.stopPropagation(); fn(); };
      return i;
    }
  }
  document.addEventListener('click', () => {
    if (state.tmplOpen) { state.tmplOpen = false; $('#tmplmenu').classList.remove('open'); }
  });

  // ---- misc chrome ------------------------------------------------------

  function dragBy(handle, target) {
    handle.onmousedown = (e) => {
      const r = target.getBoundingClientRect();
      const dx = e.clientX - r.left;
      const dy = e.clientY - r.top;
      const move = (ev) => {
        target.style.left = ev.clientX - dx + 'px';
        target.style.top = ev.clientY - dy + 'px';
      };
      const up = () => { document.removeEventListener('mousemove', move); document.removeEventListener('mouseup', up); };
      document.addEventListener('mousemove', move);
      document.addEventListener('mouseup', up);
    };
  }

  document.querySelectorAll('#left .rail-btn').forEach((b) => {
    b.onclick = () => {
      document.querySelectorAll('#left .rail-btn').forEach((x) => x.classList.remove('on'));
      b.classList.add('on');
      const t = b.dataset.tool;
      if (typeOf(t)) spawn(t);
    };
  });

  // Top-toolbar buttons that own a modal open it directly, like in the app.
  document.querySelectorAll('#top [data-act]').forEach((b) => {
    if (appOf(b.dataset.act)) b.onclick = () => openApp(b.dataset.act);
  });
  $('#top [data-act="indicators"]').onclick = () => openApp('indicator_settings');
  $('#top [data-act="symbol"]').onclick = () => openApp('search');
  $('#top [data-act="layout"]').onclick = () => openApp('tags_tabs');
  document.querySelectorAll('#right .rail-btn').forEach((b) => {
    const map = { watchlist: 'watchlist', alerts: 'alerts', trading: 'trading' };
    const id = map[b.dataset.panel];
    if (id) b.onclick = () => openApp(id);
  });

  // The launcher exists because the sim has no chart to reach things through:
  // one place that lists EVERY modal, object-bound and toolbar-owned alike.
  $('#launcher').onclick = (e) => {
    e.stopPropagation();
    const menu = $('#menu');
    const open = menu.classList.toggle('open');
    if (!open) return;
    menu.innerHTML = '';
    menu.appendChild(el('div', 'mhead', 'Модалки объектов'));
    TYPES.forEach((t) => {
      const i = el('div', 'item');
      i.appendChild(iconNode(t.id, 14));
      i.appendChild(el('span', null, `${t.name} <em>${t.spec.tabs.map((x) => x.label).join(' · ')}</em>`));
      i.onclick = () => { menu.classList.remove('open'); const id = spawn(t.id); openModal(id); };
      menu.appendChild(i);
    });
    menu.appendChild(el('div', 'mhead', 'Модалки приложения'));
    APP_MODALS.forEach((mm) => {
      const i = el('div', 'item');
      i.appendChild(iconNode(mm.icon, 14));
      i.appendChild(el('span', null, `${mm.name} <em>${mm.tabs.map((x) => x.label).join(' · ')}</em>`));
      i.onclick = () => { menu.classList.remove('open'); openApp(mm.id); };
      menu.appendChild(i);
    });
    const r = $('#launcher').getBoundingClientRect();
    menu.style.left = Math.min(r.left, window.innerWidth - 320) + 'px';
    menu.style.top = r.bottom + 4 + 'px';
  };
  document.addEventListener('click', () => $('#menu').classList.remove('open'));

  $('#bot').querySelector('[data-act="settings"]').onclick = () => { if (state.selected) openModal(state.selected); };
  $('#bot').querySelector('[data-act="del"]').onclick = () => {
    state.objects = state.objects.filter((o) => o.id !== state.selected);
    state.selected = null;
    render();
  };

  function renderScales() {
    const p = $('#pscale');
    if (!p.childElementCount) {
      for (let i = 0; i < 12; i++) {
        const d = el('div', null, (65400 - i * 100).toLocaleString('ru-RU') + '.00');
        d.style.height = 'calc((100% ) / 12)';
        p.appendChild(d);
      }
      const t = $('#tscale');
      ['7.08', '8.08', '9.08', '10.08'].forEach((s) => t.appendChild(el('span', null, s)));
    }
  }

  function tick() {
    const d = new Date();
    $('#clock').textContent = '[UTC+0] ' + d.toISOString().slice(11, 19);
  }
  setInterval(tick, 1000);

  // ---- url state --------------------------------------------------------

  function syncUrl() {
    const u = new URL(location.href);
    if (state.modal && state.modal.kind === 'type') {
      u.searchParams.set('open', 'settings');
      u.searchParams.set('type', objOf(state.modal.objId).typeId);
      u.searchParams.set('tab', state.modal.tab);
    } else if (state.modal) {
      u.searchParams.set('open', state.modal.id);
      u.searchParams.delete('type');
      u.searchParams.set('tab', state.modal.tab);
    } else {
      ['open', 'type', 'tab'].forEach((k) => u.searchParams.delete(k));
    }
    history.replaceState(null, '', u);
  }

  function fromUrl() {
    const q = new URLSearchParams(location.search);
    const open = q.get('open');
    if (!open) return;
    if (open === 'settings' && q.get('type')) {
      const id = spawn(q.get('type'));
      openModal(id, q.get('tab') || undefined);
    } else if (appOf(open)) {
      openApp(open, q.get('tab') || undefined);
    }
  }

  // ---- render + control plane -------------------------------------------

  function render() {
    renderTree();
    renderObjects();
    renderModal();
    const o = objOf(state.selected);
    const nameSlot = $('#sel-name');
    nameSlot.textContent = o ? o.name : 'Ничего не выбрано';
    paintIcons();
  }

  renderScales();
  tick();

  // The tree starts POPULATED — one object of every type. There is no chart to
  // draw them on, so an empty tree would mean hunting for a spawner before you
  // can look at anything; the point of the sim is that every modal is one click
  // away. `?empty=1` starts blank if you want the spawner path instead.
  function spawnAll() {
    TYPES.forEach((t) => spawn(t.id));
    state.selected = state.objects[0] ? state.objects[0].id : null;
    render();
  }

  render();
  if (!new URLSearchParams(location.search).has('empty')) spawnAll();
  fromUrl();
  openSide(true);

  /**
   * Agent control plane. Mirrors what agent3l/dig2browser expose, in plain JS on
   * `window`, consumed through CDP eval — the page never listens on a socket.
   */
  window.__SIM = {
    types: () => TYPES.map((t) => ({ id: t.id, name: t.name, tabs: t.spec.tabs.map((x) => x.label) })),
    spawn,
    select: (id) => { state.selected = id; render(); return id; },
    open: (objId, tab) => { openModal(objId, tab); return state.modal; },
    openType: (typeId, tab) => { const id = spawn(typeId); openModal(id, tab); return id; },
    close: closeModal,
    tab: (id) => { if (state.modal) { state.modal.tab = id; syncUrl(); render(); } },
    templates: (typeId) => templatesFor(typeOf(typeId)),
    apply: (name) => { if (state.modal) { state.current[state.modal.objId] = name; render(); } return name; },
    app: openApp,
    spawnAll,
    modals: () => ({
      objects: TYPES.map((t) => t.id),
      application: APP_MODALS.map((m) => m.id),
    }),
    snapshot: () => ({
      objects: state.objects,
      selected: state.selected,
      modal: state.modal && {
        object: state.modal.kind === 'type' ? objOf(state.modal.objId).name : appOf(state.modal.id).name,
        type: state.modal.kind === 'type' ? objOf(state.modal.objId).typeId : state.modal.id,
        tab: state.modal.tab,
        tabs: [...document.querySelectorAll('.modal .tabs .tab')].map((t) => t.textContent),
        groups: [...document.querySelectorAll('.modal .body .group > .gh')].map((g) => g.textContent.trim()),
        rows: document.querySelectorAll('.modal .body .row').length,
        applied: state.current[state.modal.objId] || null,
      },
      side: $('#side').classList.contains('open'),
    }),
    // Every length the chrome uses, read back from the live computed styles —
    // this is what a pixel check compares against the Rust constants.
    metrics: () => {
      const cs = getComputedStyle(document.documentElement);
      const out = {};
      for (const name of ['--mlc-layout-top-toolbar-height', '--mlc-layout-left-toolbar-width',
        '--mlc-layout-right-toolbar-width', '--mlc-layout-bottom-toolbar-height',
        '--mlc-layout-right-sidebar-width', '--mlc-layout-price-scale-width',
        '--mlc-layout-time-scale-height', '--mlc-shell-header-h', '--mlc-shell-footer-h',
        '--mlc-shell-tab-bar-h', '--mlc-ps-lv-row-h']) out[name] = cs.getPropertyValue(name).trim();
      const m = document.querySelector('.modal');
      if (m) {
        out.modal_rect = m.getBoundingClientRect().toJSON();
        out.head_h = document.querySelector('.modal .head').getBoundingClientRect().height;
        out.tabs_h = document.querySelector('.modal .tabs').getBoundingClientRect().height;
        out.foot_h = document.querySelector('.modal .foot').getBoundingClientRect().height;
      }
      return out;
    },
  };
})();
