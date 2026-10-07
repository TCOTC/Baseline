/* 基线的窗口行为。
 *
 * Rust 渲染整页，只有时间线这一个列表交给这里。原因只有一个：**虚拟滚动**。
 * 它要知道滚到哪儿了、总共多高、该画哪几十行，而 Rust 那边出的是一整串静态 HTML，
 * 这件事插不进去。行的 class 仍然全部来自 view.css，所以样式还是只有一份——
 * 搬过来的只是行的拼装。
 *
 * 其余都是窗口外壳的事：顶栏按钮、底部输入框、新建目标的对话。
 * 产品逻辑（什么算推进、累积怎么算、快照什么时候冻结）一行都不在这里。
 */
(function () {
  'use strict';

  // ---------------------------------------------------------------- 基础

  // 窗口没有系统边框也就没有开发工具，异常不主动送出来就等于不存在。
  function report(what, e) {
    var msg = what + ': ' + ((e && (e.stack || e.message)) || e);
    try {
      if (window.__TAURI__) {
        fetch('/__jslog', { method: 'POST', body: msg, keepalive: true });
      } else {
        if (window.console) console.error(msg);
      }
    } catch (_) {}
  }
  window.addEventListener('error', function (e) { report('window.onerror', e.message); });
  window.addEventListener('unhandledrejection', function (e) { report('unhandled', e.reason); });

  var T = window.__TAURI__;
  function invoke(cmd, args) {
    if (!T || !T.core) return Promise.reject(new Error('不在窗口里，命令发不出去'));
    return T.core.invoke(cmd, args);
  }
  function $(id) { return document.getElementById(id); }
  function esc(s) {
    return String(s).replace(/[&<>"]/g, function (c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c];
    });
  }

  // ---------------------------------------------------------------- 顶栏

  function initTitleBar() {
    if (!T || !T.window) return;
    var min = $('w-min'), max = $('w-max'), close = $('w-close');
    if (!min || !max || !close) return; // 导出的单文件没有顶栏
    var w = T.window.getCurrentWindow();

    // 最大化之后那个方框必须变成「还原」，否则它就在骗人。
    function sync() {
      w.isMaximized().then(function (on) {
        max.textContent = on ? '\uE923' : '\uE922';
      }).catch(function (e) { report('isMaximized', e); });
    }
    min.onclick = function () { w.minimize().catch(function (e) { report('minimize', e); }); };
    // 用 maximize/unmaximize 而不是 toggleMaximize：图标要跟着状态走，
    // 而状态本来就得查一次，顺带把「查」和「改」绑在同一次判断里。
    max.onclick = function () {
      w.isMaximized().then(function (on) {
        return on ? w.unmaximize() : w.maximize();
      }).catch(function (e) { report('maximize', e); });
    };
    close.onclick = function () { w.close().catch(function (e) { report('close', e); }); };
    w.onResized(sync);
    sync();
  }

  // ---------------------------------------------------------------- 流水

  /* 行高必须是固定值——虚拟滚动靠它算总高和偏移。值只有 view.css 一份，
     这里读出来用，两边不会各写一份。 */
  var ROW = { d: 34, e: 46 };
  var OVERSCAN = 8;

  var logEl, canvasEl, rowsEl, rows = [], offsets = [], lastKey = '';

  function readRowHeights() {
    var cs = getComputedStyle(document.documentElement);
    var d = parseFloat(cs.getPropertyValue('--row-day'));
    var e = parseFloat(cs.getPropertyValue('--row-entry'));
    // 解析不出来就退回默认值：宁可行高差几像素，也不要整条流水不显示。
    if (isFinite(d) && d > 0) ROW.d = d;
    if (isFinite(e) && e > 0) ROW.e = e;
  }

  function rowHeight(r) { return r.k === 'd' ? ROW.d : ROW.e; }

  function rowHtml(r) {
    if (r.k === 'd') {
      // 具体年月日在 title 上，悬停才出现。看得见的是「哪天」+「周几」。
      // title 挂在 .dlabel 上而不是整行 .day 上：那一行是通栏的，
      // 挂在行上会让「鼠标随便停在右边空白」也弹日期。
      return '<div class="day"><div class="dlabel" title="' + esc(r.title || '') + '">' +
        '<span class="dl">' + esc(r.label) + '</span>' +
        (r.wd ? '<span class="dw">' + esc(r.wd) + '</span>' : '') + '</div></div>';
    }
    // 一条记录可以挂多个目标。挂着的都列出来；`counts` 为假表示这条规则
    // 不接受手工记录——标签画成虚线并说明，免得以为它推动了那条线。
    var tags = (r.goals || []).map(function (g) {
      // 补判还没回来时不下结论：既不说「计入」，也不说「不计入」——
      // 那两句话此刻都还不成立，说了就是把没定的事说成定了。
      var no = (g.counts || r.aiwait) ? '' : ' noscore';
      var wait = r.aiwait ? ' pending' : '';
      // 悬停里说清两件事：这条规则收不收手工记录，以及这条归属是谁定的。
      // 后者不进可见文本——每行都挂一句「AI 选的」会变成噪音，但快照过了今天就冻住，
      // 一条归错的记录事后改不回来，这个事实不该消失。
      var tip = g.counts
        ? (g.byAi ? 'AI 挑的规则' : '')
        : '这条规则不接受手工记录，记了也不动这条线';
      return '<span class="chip ' + esc(g.color) + no + wait + '"' +
        (tip ? ' title="' + esc(tip) + '"' : '') + '>' + esc(g.title) + '</span>';
    }).join('');
    var none = tags ? '' : ' unlinked';
    if (!tags) {
      tags = r.aiwait
        ? '<span class="chip none pending">还没关联目标</span>'
        : '<span class="chip none">没关联目标</span>';
    }
    // 补判的状态写在**这一行**上。以前它写在输入框里（占位符变成「AI 正在分类…」，
    // 框还描红），那是把一条记录的状态安在了下一条记录的位置上。
    var sub = r.sub;
    if (r.aiwait) sub += ' • AI 正在分类…';
    else if (r.ainote) sub += ' • ' + r.ainote;
    return '<div class="entry' + none + '">' +
      '<div class="time">' + esc(r.time) + '</div>' +
      '<div class="rail"><span class="dot ' + esc((r.goals[0] || {}).color || 'none') +
      '"></span></div>' +
      '<div class="body"><div class="act" title="' + esc(r.text) + '">' + esc(r.text) + '</div>' +
      // 规则名可能很长，而行高是定死的（虚拟滚动靠它算偏移），所以这里只显示一行、
      // 超长省略，全文挂在 title 上——和上面那句备注同一个规矩。
      '<div class="sub" title="' + esc(sub) + '">' + esc(sub) + '</div></div>' +
      '<div class="tags">' + tags + '</div></div>';
  }

  /// 改一行的显示状态，然后重画。**只动这一行，不整页重来**——
  /// 后台的补判不该把用户正在写的下一条冲掉。
  function patchRow(id, wait, note) {
    for (var i = 0; i < rows.length; i++) {
      if (rows[i].k !== 'e' || rows[i].id !== id) continue;
      rows[i].aiwait = !!wait;
      if (note) rows[i].ainote = note;
      lastKey = ''; // 可见范围没变、内容变了，缓存那个判断会让我们什么都不画
      paint();
      return;
    }
  }

  function build() {
    offsets = new Array(rows.length + 1);
    offsets[0] = 0;
    for (var i = 0; i < rows.length; i++) offsets[i + 1] = offsets[i] + rowHeight(rows[i]);
    canvasEl.style.height = offsets[rows.length] + 'px';
  }

  // 二分找某一行。行有几千条时线性扫描会把每一次滚动都拖慢。
  function indexAt(y) {
    var lo = 0, hi = rows.length;
    while (lo < hi) {
      var mid = (lo + hi) >> 1;
      if (offsets[mid + 1] <= y) lo = mid + 1; else hi = mid;
    }
    return Math.max(0, Math.min(lo, rows.length - 1));
  }

  var pending = false;
  function paint() {
    pending = false;
    if (!rows.length) return;
    var top = logEl.scrollTop, vh = logEl.clientHeight;
    var start = Math.max(0, indexAt(top) - OVERSCAN);
    var end = Math.min(rows.length, indexAt(top + vh) + 1 + OVERSCAN);
    var key = start + ':' + end;
    if (key === lastKey) return; // 可见范围没变就不重排
    lastKey = key;
    var html = '';
    for (var i = start; i < end; i++) html += rowHtml(rows[i]);
    rowsEl.style.transform = 'translateY(' + offsets[start] + 'px)';
    rowsEl.innerHTML = html;
  }

  function schedule() {
    if (pending) return;
    pending = true;
    requestAnimationFrame(paint);
  }

  function scrollToBottom() {
    logEl.scrollTop = logEl.scrollHeight;
    paint();
  }

  function initLog() {
    logEl = $('log');
    canvasEl = $('log-canvas');
    rowsEl = $('log-rows');
    if (!logEl || !canvasEl || !rowsEl) return;
    readRowHeights();

    try {
      rows = JSON.parse($('log-data').textContent) || [];
    } catch (e) {
      report('log-data 解析失败', e);
      rows = [];
    }

    if (!rows.length) {
      canvasEl.style.height = 'auto';
      rowsEl.style.position = 'static';
      rowsEl.innerHTML = '<div class="log-hint">还没有任何记录。<br>' +
        '下面那个输入框里写一句，回车——记的是「刚做了什么」。</div>';
      return;
    }

    build();
    // 下新上旧：一进来就该看到最新的那条，所以先落到底。
    scrollToBottom();
    logEl.addEventListener('scroll', schedule, { passive: true });
    window.addEventListener('resize', function () { lastKey = ''; schedule(); });
  }

  // ---------------------------------------------------------------- 输入框

  function initComposer() {
    var form = $('composer');
    if (!form) return; // 导出的单文件没有输入框
    var input = $('composer-input');
    var field = $('composer-goals');
    var chips = $('gchips');
    var more = $('gmore');
    var gmenu = $('gmenu');
    var rules = $('rules');
    var rlist = $('rules-list');

    // 每个目标下能收手工记录的规则，由 Rust 连**会自动归属的那一条**一起列好
    // （见 render::composer_html）。`auto` 就是内核 db::pick_source 的答案。
    // **这里只负责画和问，不负责判**：归属仍然由内核定。
    var ruleMap = {};
    try {
      var rulesEl = $('composer-rules');
      ruleMap = rulesEl ? JSON.parse(rulesEl.textContent) : {};
    } catch (e) {
      report('composer-rules', e);
    }

    // AI 能不能用。能用就**不要在本地拦下「没选规则」的提交**——
    // 拦下来模型就永远没机会回答，而「不选也能记」正是这个功能的全部意义。
    var aiOn = (($('composer-ai') || {}).value === '1');

    // 存在 sessionStorage 里的两件东西。用 session 而不是 URL 或 localStorage：
    // 它们天生是「这一趟的事」——补判的待办做完就没用了，草稿也不该活过这次运行。
    var DRAFT = 'bl:draft';
    var AIQ = 'bl:aiq';

    // 默认两行、随内容长高。
    var MAX_H = 160;
    function grow() {
      input.style.height = 'auto';
      input.style.height = Math.min(input.scrollHeight, MAX_H) + 'px';
    }
    input.addEventListener('input', grow);
    input.dataset.ph = input.placeholder; // 出错提示要能还原回去
    grow();

    // ---- 这条记录推进了哪些目标 ----
    //
    // 全部目标都列出来，点一下选中、再点一下取消，可以多选，也可以一个都不选。
    // **默认一个都不选**：记下来是第一步，归到哪个目标是第二步；
    // 逼着先选目标，等于在「我还不知道这算推进什么」的时候替人做决定。
    //
    // 状态只有 `chosen` 一份，标签和菜单都从它画出来——两处各存一份迟早对不上。
    var chosen = {};
    function ids() { return Object.keys(chosen); }

    // 人点过的「这条记录算哪条规则」，键是目标 id。
    //
    // 选中目标之后，每个目标都会先按内核算出的 `auto` 预置一条——
    // **能唯一确定的那条直接就是选中的状态**，于是它在按下回车前就看得见。
    // 两条以上时 `auto` 是 null，气泡全是空的，必须点一个才发得出去。
    var picked = {};

    // 还没指认规则的选中目标。null 表示都指认好了。
    function missingPick() {
      var out = null;
      ids().forEach(function (gid) {
        var r = ruleMap[gid];
        if (r && r.sources.length > 1 && !picked[gid]) out = out || gid;
      });
      return out;
    }

    // 把选中目标下的规则画成一排气泡，左对齐，就在输入区上面。
    function refreshRules() {
      if (!rules || !rlist) return;

      // 取消勾选的目标，它的规则气泡跟着消失。
      Object.keys(picked).forEach(function (gid) {
        if (!chosen[gid]) delete picked[gid];
      });

      var groups = [];
      ids().forEach(function (gid) {
        var r = ruleMap[gid];
        if (!r || !r.sources.length) return;
        // 内核说这条唯一可归属 —— 直接预置成选中，不用人再点一下。
        if (r.auto && !picked[gid]) picked[gid] = r.auto;
        groups.push({ goal: gid, title: r.title, color: r.color, list: r.sources });
      });

      rules.hidden = groups.length === 0;
      // 只有一个目标时不必写它的名字：下面那排目标气泡里已经亮着它了。
      rules.classList.toggle('many', groups.length > 1);
      rlist.innerHTML = '';
      groups.forEach(function (g) {
        if (groups.length > 1) {
          var h = document.createElement('span');
          h.className = 'rgroup ' + g.color;
          h.textContent = g.title;
          rlist.appendChild(h);
        }
        g.list.forEach(function (s) {
          var b = document.createElement('button');
          b.type = 'button';
          b.className = 'rchip ' + g.color + (picked[g.goal] === s.id ? ' on' : '');
          b.dataset.goal = g.goal;
          b.dataset.src = s.id;
          b.textContent = s.what;
          b.title = '规则 #' + s.id + '：' + s.what;
          rlist.appendChild(b);
        });
      });
    }

    function sync() {
      if (field) field.value = ids().join(',');
      Array.prototype.forEach.call(document.querySelectorAll('.gchip'), function (b) {
        b.classList.toggle('on', !!chosen[b.dataset.goal]);
      });
      Array.prototype.forEach.call(document.querySelectorAll('.gopt'), function (b) {
        b.classList.toggle('on', !!chosen[b.dataset.goal]);
      });
      if (more) more.classList.toggle('on', ids().length > 0);
      refreshRules();
    }

    // 一行放不下就整排收起来，只留一个「…」。用真实的布局宽度判断，
    // 不靠猜字符数——目标名有长有短，猜不准。
    function fitChips() {
      if (!chips || !more) return;
      chips.hidden = false;
      more.hidden = true;
      if (chips.scrollWidth > chips.clientWidth + 1) {
        chips.hidden = true;
        more.hidden = false;
      }
    }

    function toggle(id) {
      if (!id) return;
      if (chosen[id]) delete chosen[id]; else chosen[id] = true;
      sync();
      fitChips();
      // 点完标签把光标还给输入框：选目标是记录的一个动作，
      // 不该让「选完还要再点一下输入框才能打字」变成必须知道的事。
      input.focus();
    }

    if (chips) {
      chips.addEventListener('click', function (ev) {
        var b = ev.target.closest('.gchip');
        if (b) toggle(b.dataset.goal);
      });
    }
    if (gmenu) {
      gmenu.addEventListener('click', function (ev) {
        var b = ev.target.closest('.gopt');
        if (b) toggle(b.dataset.goal);
      });
    }
    if (rlist) {
      rlist.addEventListener('click', function (ev) {
        var b = ev.target.closest('.rchip');
        if (!b) return;
        var g = b.dataset.goal, s = Number(b.dataset.src);
        // 一个目标下只能算一条规则，所以这里不是多选：点另一条就换过去。
        // 再点当前这条 = 取消（「这条不算它」），留一条退路。
        if (picked[g] === s) delete picked[g]; else picked[g] = s;
        refreshRules();
        light(rules, !!missingPick()); // 指认过了就不再拦着
        input.focus();
      });
    }
    if (more && gmenu) {
      more.addEventListener('click', function () { gmenu.hidden = !gmenu.hidden; });
      // 点别处、按 Esc 都收起来：一个不会消失的浮层会挡住它下面的东西。
      document.addEventListener('click', function (ev) {
        if (!more.contains(ev.target) && !gmenu.contains(ev.target)) gmenu.hidden = true;
      });
      document.addEventListener('keydown', function (ev) {
        if (ev.key === 'Escape') gmenu.hidden = true;
      });
    }
    window.addEventListener('resize', fitChips);
    sync();
    fitChips();

    var hintT = 0;
    function hint(msg) {
      input.placeholder = msg;
      form.classList.add('broke');
      clearTimeout(hintT);
      hintT = setTimeout(function () {
        form.classList.remove('broke');
        input.placeholder = input.dataset.ph || '';
        grow();
      }, 4000);
    }

    // 让某个元素自己亮起来。**提示不能只写在 placeholder 上**——
    // 输入框里有字的时候 placeholder 根本看不见，而「两条规则都能收它」
    // 恰恰总是在写完一句话、按下回车的那一刻才遇到。
    //
    // 亮到人真的动手为止（选了一条规则就灭），不是闪一下就没了：
    // 闪一下的东西会被当成没看见，而这是唯一挡住这次记录的坎。
    function light(el, on) {
      if (!el) return;
      el.classList.toggle('broke', on !== false);
    }

    function fail(e) {
      form.classList.remove('busy');
      input.disabled = false;
      report('add_checkin', e);
      // 失败原因是「规则没定」时，把那排气泡也点亮——不然只有输入框描红，
      // 该动手的地方却安安静静。
      if (missingPick()) light(rules);
      hint('没记上：' + ((e && e.message) || e));
    }

    function send() {
      var text = input.value.trim();
      if (!text || form.classList.contains('busy')) return;
      // 两条规则都能收这条记录时必须先指认，但**只在没人接手的时候**才拦：
      // AI 能用就放它过去——内核会先落库，界面回头再叫它补判。
      if (missingPick() && !aiOn) {
        refreshRules();
        light(rules); // 要动手的是这排气泡，让它自己说话
        hint('选一条规则再记');
        return;
      }
      form.classList.add('busy');
      input.disabled = true;
      // 一个都没选就是 []，Tauri 那边收到空数组 —— 这条记录不关联任何目标。
      var picks = [];
      Object.keys(picked).forEach(function (g) { picks.push([Number(g), picked[g]]); });
      invoke('add_checkin', { goalIds: ids().map(Number), note: text, picks: picks })
        .then(function (r) {
          // 记录已经落库了，立刻重来让人看见。**要补判的留给下一次加载**：
          // 这一次要是等模型，就又变回「点了没反应，过一会儿才刷新」。
          try {
            if (r && r.needsAi) sessionStorage.setItem(AIQ, String(r.id));
          } catch (e) { /* 存不下只是这次不自动补判，记录照样在 */ }
          location.reload();
        })
        .catch(fail);
    }

    // ---- 补判：上一次记完留下的待办 ----
    //
    // 记录先落库、立刻可见，所以这一步**没有时间压力**：成了就把归属补上，
    // 不成那条记录也还在，列表上还能再叫它一次。
    //
    // **状态写在那一条记录上，输入框一概不碰。** 这一趟可能是几秒、也可能超时，
    // 期间人大概率已经在写第二条了——把「正在分类」塞进输入框，
    // 等于用下一条记录的位置去播上一条的状态。
    function drainAiQueue() {
      var id = null;
      try {
        id = sessionStorage.getItem(AIQ);
        sessionStorage.removeItem(AIQ);
      } catch (e) { report('ai-queue', e); }
      if (!id) return;
      id = Number(id);
      patchRow(id, true);

      // 这一条此刻确实「没关联目标」，但它是**正在办**的待办，不是积压。
      // 那个「有 N 条没关联目标」的提示把它一起数进去就是句不准确的话——
      // 先收起来，判成了整页重来它会自己消失，判不成再放回去（那时它是真的还悬着）。
      var unwarn = $('unlinked-warn');
      if (unwarn) unwarn.hidden = true;

      invoke('classify_checkin', { checkinId: id })
        .then(function (msg) {
          if (msg && msg.indexOf('归了') > -1) {
            // 归属落库了，卡片上的数字和曲线都跟着变 —— 只能整页重来，
            // 界面上没有第二份渲染能算出那条曲线。
            keepDraft();
            location.reload();
          } else {
            // 没判出来是正常结果，不是错误：那面标签本来就写着「·不计入」。
            patchRow(id, false, 'AI 没判出来');
            if (unwarn) unwarn.hidden = false;
          }
        })
        .catch(function (e) {
          patchRow(id, false, 'AI 没判成');
          if (unwarn) unwarn.hidden = false;
          report('ai-classify', e);
        });
    }

    // 整页重来会把没提交的字丢掉。补判回来时也会重来一次，而那时人很可能
    // 已经在写下一条了——**连着光标位置一起**存下来，重来之后放回去。
    // 只回填文字不回填光标，等于把人的光标甩到开头，接着打就会插错地方。
    function keepDraft() {
      try {
        if (input.value.trim()) {
          sessionStorage.setItem(DRAFT, JSON.stringify({
            v: input.value,
            s: input.selectionStart,
            e: input.selectionEnd
          }));
        } else {
          sessionStorage.removeItem(DRAFT);
        }
      } catch (e) { /* 存不下就算了，不该因此挡住记录 */ }
    }

    function restoreDraft() {
      try {
        var raw = sessionStorage.getItem(DRAFT);
        if (!raw) return;
        sessionStorage.removeItem(DRAFT);
        var d = JSON.parse(raw);
        input.value = d.v || '';
        grow();
        if (typeof d.s === 'number') {
          try { input.setSelectionRange(d.s, d.e); } catch (e) { /* 老引擎不支持就算了 */ }
        }
      } catch (e) { /* 同上 */ }
    }
    restoreDraft();

    form.addEventListener('submit', function (ev) { ev.preventDefault(); send(); });
    // 回车提交、Shift+回车换行。支持多行不该让「写完一句回车」变成按两个键。
    input.addEventListener('keydown', function (ev) {
      if (ev.key === 'Enter' && !ev.shiftKey) {
        ev.preventDefault();
        send();
      }
    });

    focusComposer();
    // 补判排在最后：它可能会触发整页重来，而重来之前这一页该初始化完的都初始化完了。
    // 放在这里而不是启动那一段，是因为它要用输入框的状态（草稿、提示语），
    // 而那些东西的作用域就在这个函数里。
    drainAiQueue();
  }

  function focusComposer() {
    var input = $('composer-input');
    if (!input || input.disabled) return;
    try { input.focus(); } catch (_) {}
  }
  // 托盘左键点开窗口之后由外壳调用，把光标直接放进输入框。
  window.__blFocusComposer = focusComposer;

  // ---------------------------------------------------------------- 新建目标的对话

  /* 骨架是固定的四步 + 一步收口。
   *
   * 为什么要固定：设计文档 §11.1 那条「3 轮上限怎么定」之所以难调，
   * 是因为自由聊天只能靠 prompt 求它自觉。步数就是步数，就不用调了。
   * 而且 **「什么不算」这一问永远不会被跳过**——它是最容易跳过、也最值钱的一问，
   * 交给自由聊天就是碰运气。
   *
   * AI 在这里没有位置：对话要问什么是产品定死的，判据（≥1 条来源、全部可计算、
   * 当场算出当前值）也全是机械的。等真需要把一句话拆成几条来源时再说。 */
  var KINDS = [
    { k: 'manual_checkin', name: '手工打卡', desc: '我自己记一次。比如「读完一章算一次」', need: false },
    { k: 'git_commits', name: 'git 提交', desc: '某个仓库的提交数。还没接入，先登记规则', need: true, ask: '哪个仓库？（先填名字，接上之后才会计数）' },
    { k: 'external_metric', name: '外部数据', desc: '读别处已经记着的数，比如复习记录', need: true, ask: '读哪份数据？（先填标识，接上之后才会计数）' },
    { k: 'derived', name: '组合', desc: '上面几条按公式算', need: false }
  ];

  var dlgEl, d = null;

  var STEP_NAMES = ['想推进什么', '为什么是现在', '什么算推进它', '什么不算', '落到哪种来源'];

  function dlgHead() {
    var h = '<div class="dlg-steps">';
    for (var i = 0; i < STEP_NAMES.length; i++) {
      var cls = i < d.step ? 'done' : (i === d.step ? 'now' : '');
      h += '<div class="' + cls + '">' + (i + 1) + ' · ' + esc(STEP_NAMES[i]) + '</div>';
    }
    return h + '</div>';
  }

  function draw() {
    var h = dlgHead();
    var i;
    // 回车该干什么，由状态说了算。**绑在容器上、只绑一次**——
    // 绑在每个重建出来的输入框上，重建一次监听就没了（踩过：文字进去了，
    // 回车没反应，看着像「键盘不灵」）。
    d.enter = next;

    if (d.step === 0) {
      h += '<h2>你想推进什么？</h2>' +
        '<p class="note">一句话就行。它会是左栏那张卡的标题。</p>' +
        '<input type="text" id="dlg-in" maxlength="40" placeholder="比如：打牢计算机基础">' +
        dlgNav();
    } else if (d.step === 1) {
      h += '<h2>为什么是现在？</h2>' +
        '<p class="note">遇到什么事了，还是在准备什么？<br>' +
        '这一问是为了推出「什么算推进它」，<b>不是问你感受</b>——所以只有在你上一句太宽、' +
        '推不出规则的时候才问。</p>' +
        '<input type="text" id="dlg-in" maxlength="120" placeholder="比如：想做出自己的产品，但基础不够">' +
        dlgNav();
    } else if (d.step === 2) {
      h += '<h2>什么算推进它？</h2>' +
        '<p class="note">越具体越好。一句能装下任何活动的规则等于没有规则——' +
        '那样的曲线会一直涨，也就永远不告诉你偏了。</p>' +
        '<input type="text" id="dlg-in" maxlength="120" placeholder="比如：读完一章，或做完一章题，算一次">' +
        dlgNav();
    } else if (d.step === 3) {
      h += '<h2>什么不算？</h2>' +
        '<p class="note">最容易跳过、也最值钱的一问。<br>' +
        '同一件事看起来像在推进它、其实不算的，是什么？<br>' +
        '（例：在 Learn-English 上写代码不算推进「英语」——只算复习记录。<br>' +
        '不划这条边界，那条曲线会假装在涨。）</p>' +
        '<input type="text" id="dlg-in" maxlength="120" placeholder="比如：在它上面写代码、改功能，不算">' +
        '<div class="row" style="margin-bottom:10px">' +
        '<button type="button" id="dlg-skip">想不出来，先跳过</button></div>' +
        dlgNav();
    } else if (d.step === 4) {
      h += '<h2>这条规则落到哪种来源？</h2>' +
        '<p class="note">四种，是上限。落到别的上去，它就开始变成另一件东西了。</p>';
      for (i = 0; i < KINDS.length; i++) {
        h += '<button type="button" class="kind" data-kind="' + KINDS[i].k + '">' +
          '<b>' + esc(KINDS[i].name) + '</b><span>' + esc(KINDS[i].desc) + '</span></button>';
      }
      h += '<div class="row" style="margin-top:12px"><button type="button" id="dlg-cancel">取消</button></div>';
      if (d.problem) h += '<div class="cur bad">' + esc(d.problem) + '</div>';
    } else if (d.step === 5) {
      var k = kindOf(d.kind);
      h += '<h2>确认一下</h2>' +
        '<div class="answer"><b>' + esc(d.title) + '</b></div>' +
        (d.why ? '<div class="answer">为什么：' + esc(d.why) + '</div>' : '') +
        '<div class="answer">算推进：' + esc(d.rule) + '</div>' +
        '<div class="answer">不算：' + (d.notrule ? esc(d.notrule) : '<b>没划</b>') + '</div>' +
        '<div class="answer">来源：' + esc(k.name) + (d.target ? ' · ' + esc(d.target) : '') + '</div>' +
        (d.notrule ? '' : '<div class="cur bad">没划边界。以后出现「看起来像在推进它、其实不算」的' +
          '活动时，这条曲线会说谎——而它会理直气壮地涨。</div>') +
        '<div class="row" style="margin-top:16px">' +
        '<button type="button" class="primary" id="dlg-ok">建这个目标</button>' +
        '<button type="button" id="dlg-cancel">取消</button></div>' +
        (d.problem ? '<div class="cur bad">' + esc(d.problem) + '</div>' : '');
    } else if (d.step === 6) {
      h += '<h2>建好了</h2>' +
        '<div class="cur">当前值 <b>' + d.value + '</b> 次<br>' +
        (d.value === 0
          ? '还是 0。要么规则写错了，要么这条线真的还没动——两种都值得知道。'
          : '规则一写完就算出了数，说明它是可计算的。') +
        '</div>' +
        '<div class="row" style="margin-top:16px">' +
        '<button type="button" class="primary" id="dlg-done">好</button></div>';
    }

    dlgEl.innerHTML = h;
    var input = $('dlg-in');
    if (input) {
      input.value = d['v' + d.step] || '';
      focusInput();
    }
    var skip = $('dlg-skip');
    if (skip) skip.onclick = function () { d.notrule = ''; d.step = 4; draw(); };
    // 下一步 / 上一步必须有接线。少了这两行，按钮画出来了、点了没反应——
    // 而回车又能走通，所以看起来「界面是好的，只是键盘不灵」。踩过。
    var nextBtn = $('dlg-next');
    if (nextBtn) nextBtn.onclick = next;
    var backBtn = $('dlg-back');
    if (backBtn) {
      backBtn.onclick = function () {
        if (d.step > 0) { d.problem = ''; d.step -= 1; draw(); }
      };
    }
    var ok = $('dlg-ok');
    if (ok) ok.onclick = function () { create(); };
    var done = $('dlg-done');
    if (done) done.onclick = function () { location.reload(); };
    var cancel = $('dlg-cancel');
    if (cancel) cancel.onclick = closeDialog;
    Array.prototype.forEach.call(dlgEl.querySelectorAll('button.kind'), function (b) {
      b.onclick = function () { pickKind(b.dataset.kind); };
    });
  }

  function kindOf(k) {
    for (var i = 0; i < KINDS.length; i++) if (KINDS[i].k === k) return KINDS[i];
    return KINDS[0];
  }

  /// 把光标放进当前这一步的输入框。
  ///
  /// 不能在写完 innerHTML 之后立刻 focus：那一刻元素还没有完成布局，
  /// focus() 会被静默忽略——**不报错，只是没生效**，看起来像「键盘不灵」。
  /// 所以延到下一帧，并且再补一次：一帧之后仍然没拿到的，多半是被别的东西抢走了。
  function focusInput() {
    function go() {
      var input = $('dlg-in');
      if (!input || document.activeElement === input) return;
      try { input.focus(); } catch (_) {}
    }
    requestAnimationFrame(go);
    setTimeout(go, 80);
  }

  function dlgNav() {
    return '<div class="row">' +
      '<button type="button" class="primary" id="dlg-next">下一步</button>' +
      (d.step > 0 ? '<button type="button" id="dlg-back">上一步</button>' : '') +
      '<button type="button" id="dlg-cancel">取消</button></div>' +
      (d.problem ? '<div class="cur bad">' + esc(d.problem) + '</div>' : '');
  }

  function next() {
    var v = ($('dlg-in') && $('dlg-in').value || '').trim();
    if (d.step === 0) {
      if (!v) { d.problem = '总得先有个名字。'; draw(); return; }
      d.title = v;
    } else if (d.step === 1) {
      d.why = v; // 可以不填：不是每个目标都需要问目的
    } else if (d.step === 2) {
      if (!v) {
        // 这是「必须再问」的落点：收不出一条可计算的规则，就不该往下走。
        d.problem = '这条填不了。没有它，这个目标给不出「什么算推进它」——' +
          '曲线永远不会动，它也就不是目标，只是一句愿望。';
        draw();
        return;
      }
      d.rule = v;
    } else if (d.step === 3) {
      d.notrule = v;
    }
    d.problem = '';
    d.step += 1;
    draw();
  }

  function pickKind(k) {
    var spec = kindOf(k);
    if (spec.need) {
      // 需要 target 的来源：先问清对象。空着就等于「什么都算」，
      // 那正是设计文档里点名的坏规则。
      d.kind = k;
      d.enter = takeTarget;
      dlgEl.innerHTML = dlgHead() + '<h2>' + esc(spec.name) + ' · 具体是哪个？</h2>' +
        '<p class="note">' + esc(spec.ask) + '</p>' +
        '<input type="text" id="dlg-in" maxlength="120">' +
        '<div class="row"><button type="button" class="primary" id="dlg-next">下一步</button>' +
        '<button type="button" id="dlg-back">上一步</button>' +
        '<button type="button" id="dlg-cancel">取消</button></div>' +
        (d.problem ? '<div class="cur bad">' + esc(d.problem) + '</div>' : '');
      var input = $('dlg-in');
      input.value = d.target || '';
      focusInput();
      $('dlg-next').onclick = takeTarget;
      $('dlg-back').onclick = function () { d.step = 4; d.problem = ''; draw(); };
      $('dlg-cancel').onclick = closeDialog;
      return;
    }
    d.kind = k;
    d.target = '';
    d.problem = '';
    d.step = 5;
    draw();
  }

  function takeTarget() {
    var v = ($('dlg-in') && $('dlg-in').value || '').trim();
    if (!v) {
      d.problem = '空着就等于「什么都算」——那条规则会把所有活动都装进来，' +
        '曲线永远在涨，也就不告诉你偏了。';
      pickKind(d.kind);
      return;
    }
    d.target = v;
    d.problem = '';
    d.step = 5;
    draw();
  }

  function create() {
    var btn = $('dlg-ok');
    if (btn) btn.disabled = true;
    invoke('add_goal', {
      title: d.title, why: d.why, rationale: d.rule, kind: d.kind, target: d.target
    }).then(function (value) {
      d.value = value;
      d.problem = '';
      d.step = 6;
      draw();
    }).catch(function (e) {
      if (btn) btn.disabled = false;
      report('add_goal', e);
      d.problem = '没建成：' + ((e && e.message) || e);
      draw();
    });
  }

  function openDialog() {
    d = { step: 0, title: '', why: '', rule: '', notrule: '', kind: '', target: '',
          problem: '', enter: null, value: 0 };
    dlgEl.hidden = false;
    if (logEl) logEl.hidden = true;
    var c = $('composer');
    if (c) c.hidden = true;
    draw();
  }

  function closeDialog() {
    d = null;
    dlgEl.hidden = true;
    dlgEl.innerHTML = '';
    if (logEl) logEl.hidden = false;
    var c = $('composer');
    if (c) c.hidden = false;
    focusComposer();
  }

  function initAddGoal() {
    dlgEl = $('dlg');
    var btn = $('addgoal');
    if (!dlgEl || !btn || !T) return;
    btn.addEventListener('click', openDialog);
    dlgEl.addEventListener('keydown', function (ev) {
      if (ev.key !== 'Enter' || !d) return;
      var t = ev.target;
      if (!t || t.tagName !== 'INPUT') return;
      ev.preventDefault();
      if (d.enter) d.enter();
    });
  }

  // ---------------------------------------------------------------- 模态确认

  /* 危险动作的确认框。返回一个 Promise<boolean>。
     不用 window.confirm：原生弹窗在这扇无边框窗口里很突兀，
     而且它挡住 JS 线程，连我们自己的日志都送不出去。 */
  function confirmModal(opt) {
    return new Promise(function (resolve) {
      var modal = $('modal');
      if (!modal) { resolve(window.confirm(opt.title)); return; }
      $('m-title').textContent = opt.title || '';
      $('m-text').textContent = opt.text || '';
      var what = $('m-what');
      what.textContent = opt.what || '';
      what.hidden = !opt.what;
      var ok = $('m-ok');
      ok.textContent = opt.ok || '确定';
      ok.classList.toggle('danger', opt.danger !== false);

      function close(v) {
        modal.hidden = true;
        ok.onclick = null;
        $('m-cancel').onclick = null;
        modal.onclick = null;
        document.removeEventListener('keydown', onKey);
        resolve(v);
      }
      function onKey(ev) { if (ev.key === 'Escape') close(false); }
      ok.onclick = function () { close(true); };
      $('m-cancel').onclick = function () { close(false); };
      // 点遮罩关掉，点盒子本身不关。
      modal.onclick = function (ev) { if (ev.target === modal) close(false); };
      document.addEventListener('keydown', onKey);
      modal.hidden = false;
      $('m-cancel').focus();
    });
  }

  // ---------------------------------------------------------------- 目标详情

  /* 点卡片进来的那一屏。Rust 已经把所有状态（读的、编辑的、归档的、加规则的）
     一次渲染好了，这里只负责切显隐和把结果发回去。
     所以「一个目标长什么样」仍然只有一处定义。 */
  function initDetail() {
    // **判据必须是 `data-goal`，不能是 `.detail`。** 设置页共用同一套外壳
    // （.detail / .hero / .dsec 都是同一份 CSS），只按类名判会把它当成详情页，
    // 然后在这个页面上找不到 `#etitle` 之类的东西 —— 屏幕上是整页空白，
    // 而原因看起来像「设置页没渲染」。（踩过，靠 desktop.log 抓到的。）
    var detail = document.querySelector('.detail[data-goal]');
    if (!detail || !T) return; // 不是详情页，或者不在窗口里
    var id = Number(detail.dataset.goal);

    var view = $('dview'), edit = $('deditform'), arch = $('darchform'), addr = $('addrform');
    var err = $('derr');

    function fail(e) {
      report('detail', e);
      if (!err) return;
      err.textContent = (e && e.message) || String(e);
      err.hidden = false;
    }
    function ok() { location.reload(); }
    function back() { location.href = '/'; }

    // ---- 改目标 ----
    $('dedit').onclick = function () {
      view.hidden = true; edit.hidden = false;
      $('etitle').focus();
    };
    $('ecancel').onclick = function () { edit.hidden = true; view.hidden = false; };
    $('esave').onclick = function () {
      var title = $('etitle').value.trim();
      if (!title) { fail(new Error('目标得有个名字')); return; }
      invoke('update_goal', { id: id, title: title, why: $('ewhy').value }).then(ok).catch(fail);
    };

    // ---- 归档：必须写一句原因 ----
    $('darch').onclick = function () { arch.hidden = false; $('areason').focus(); };
    $('acancel').onclick = function () { arch.hidden = true; };
    $('asave').onclick = function () {
      var reason = $('areason').value.trim();
      if (!reason) {
        fail(new Error('归档要写一句原因——那是三个月后唯一能回看的东西'));
        return;
      }
      // 归档完它就不在列表里了，回主视图，别停在一个已经归档的目标上。
      invoke('archive_goal', { id: id, reason: reason }).then(back).catch(fail);
    };

    // ---- 删除：只有一条记录都没有时才可点（Rust 那边已经按这个置灰了）----
    var del = $('ddel');
    if (del && !del.disabled) {
      del.onclick = function () {
        confirmModal({
          title: '删掉这个目标？',
          text: '它还没有任何记录，删了就没了。如果只是暂时不想做，走归档——' +
            '曲线和记录都会留着，还有你写下的那句理由。',
          what: $('.dtitle') ? $('.dtitle').textContent.trim() : '',
          ok: '删掉',
        }).then(function (yes) {
          if (!yes) return;
          invoke('delete_goal', { id: id }).then(back).catch(fail);
        });
      };
    }

    // ---- 规则的增删 ----
    Array.prototype.forEach.call(document.querySelectorAll('.rule'), function (row) {
      var b = row.querySelector('.x');
      if (!b) return;
      b.onclick = function () {
        var kind = row.querySelector('.rkind');
        var why = row.querySelector('.rwhy');
        confirmModal({
          title: '删掉这条判定规则？',
          text: '删掉之后，这类活动就不再推进这个目标了。' +
            '如果这是最后一条规则，它的曲线会停在这里不再动。',
          what: [kind && kind.textContent.trim(), why && why.textContent.trim()]
            .filter(Boolean).join(' · '),
          ok: '删掉规则',
        }).then(function (yes) {
          if (!yes) return;
          invoke('delete_source', { id: Number(b.dataset.src) }).then(ok).catch(fail);
        });
      };
    });

    $('addrbtn').onclick = function () { addr.hidden = !addr.hidden; };
    $('arcancel').onclick = function () { addr.hidden = true; };

    var chosen = '';
    Array.prototype.forEach.call(document.querySelectorAll('#addrkinds .kind'), function (b) {
      b.onclick = function () {
        chosen = b.dataset.kind;
        Array.prototype.forEach.call(document.querySelectorAll('#addrkinds .kind'), function (x) {
          x.classList.remove('on');
        });
        b.classList.add('on');
        // 需要对象的来源（git 仓库、外部数据）必须问清是哪个。
        // 空着就等于「什么都算」，那正是设计文档点名的坏规则。
        var need = chosen === 'git_commits' || chosen === 'external_metric';
        $('addrtarget').hidden = !need;
        $('addrhint').textContent =
          chosen === 'git_commits' ? '哪个仓库？' : '读哪份数据？';
      };
    });

    $('arsave').onclick = function () {
      if (!chosen) { fail(new Error('先选一种来源')); return; }
      var t = $('artarget').value.trim();
      if ((chosen === 'git_commits' || chosen === 'external_metric') && !t) {
        fail(new Error('空着就等于「什么都算」——那条规则会把所有活动都装进来，曲线永远在涨'));
        return;
      }
      invoke('add_source', {
        goalId: id, kind: chosen, target: t, rationale: $('arrationale').value
      }).then(ok).catch(fail);
    };
  }

  // ---------------------------------------------------------------- 设置

  // 「让 AI 判这些」：那些记录已经在库里了，只是还没归好。
  // 没有这个出口，一次失败的补判就留下一个死胡同——记录取不回来也归不了。
  //
  // 两处有它，共用这一段：目标的详情页（那个目标下挂空着的），
  // 和主视图（**压根没关联目标**的那些）。`data-goal` 空着就是「全库没关联目标的」。
  function initAiRetry() {
    Array.prototype.forEach.call(document.querySelectorAll('.airetry'), function (b) {
      b.onclick = function () {
        var raw = b.dataset.goal;
        var was = b.textContent;
        b.disabled = true;
        b.textContent = '正在判…';
        invoke('classify_backlog', { goalId: raw ? Number(raw) : null })
          .then(function () { location.reload(); })
          .catch(function (e) {
            b.disabled = false;
            b.textContent = was;
            report('ai-backlog', e);
          });
      };
    });
  }

  // 齿轮：和详情页一样走**服务端路由**（?settings=1），整页由 Rust 重渲染。
  // 不在 JS 里切视图——密钥的掩码是渲染时定死的，明文根本到不了这一页。
  function initGear() {
    var g = $('gear');
    if (!g) return;
    g.onclick = function () { location.href = '?settings=1'; };
  }

  function initSettings() {
    var save = $('ai-save');
    if (!save) return; // 不在设置页

    var th = $('ai-th');
    if (th) {
      th.oninput = function () { $('ai-th-v').textContent = th.value; };
    }

    function msg(text, bad) {
      var m = $('ai-msg');
      m.textContent = text || '';
      m.classList.toggle('bad', !!bad);
    }

    // 密钥框留空 = 不动现在这把。**空着不能等于清掉**——界面上那个框本来就是空的，
    // 那样每次保存都会顺手把密钥删了。要删得按「清掉」，而且按两下。
    function payload(enabled, keyValue) {
      return {
        enabled: enabled,
        baseUrl: $('ai-base').value,
        model: $('ai-model').value,
        threshold: Number($('ai-th').value),
        apiKey: keyValue
      };
    }

    function keepKey(enabled) {
      var v = $('ai-key').value;
      return payload(enabled, v.trim() === '' ? null : v);
    }

    save.onclick = function () {
      msg('');
      invoke('save_settings', keepKey(true))
        .then(function () { location.reload(); })
        .catch(function (e) { msg('没存上：' + ((e && e.message) || e), true); });
    };

    $('ai-test').onclick = function () {
      msg('正在测…');
      // 「测一下」要先把当前填的存下来再测，否则测的是上一次的配置，
      // 而人以为测的是屏幕上这些字。
      invoke('save_settings', keepKey(true))
        .then(function () { return invoke('test_ai', {}); })
        .then(function (r) { msg(r); })
        .catch(function (e) { msg('没通：' + ((e && e.message) || e), true); });
    };

    var toggle = $('ai-toggle');
    if (toggle) {
      toggle.onclick = function () {
        msg('');
        // 按钮上是「关掉」还是「打开」由渲染时定；点下去就是取反。
        var on = toggle.textContent.indexOf('关掉') === 0;
        invoke('save_settings', keepKey(!on))
          .then(function () { location.reload(); })
          .catch(function (e) { msg('没改成功：' + ((e && e.message) || e), true); });
      };
    }

    var clr = $('ai-key-clear');
    if (clr) {
      // 两下：这个动作撤不回来（原来的密钥只在库里存着密文，删了就没了）。
      var armed = false;
      clr.onclick = function () {
        if (!armed) {
          armed = true;
          clr.classList.add('armed');
          clr.textContent = '再点一次';
          return;
        }
        msg('');
        invoke('save_settings', payload(true, ''))
          .then(function () { location.reload(); })
          .catch(function (e) { msg('没清掉：' + ((e && e.message) || e), true); });
      };
    }
  }

  // ---------------------------------------------------------------- 启动

  try {
    initTitleBar();
    initLog();
    initComposer();
    initAddGoal();
    initDetail();
    initGear();
    // 补判的重试按钮主视图和详情页都有，所以单独一段，不挂在 initDetail 里面
    // （那个函数在没有 `data-goal` 的页面上会直接返回，主视图上的按钮就没人接了）。
    initAiRetry();
    // 设置页排在 initDetail 之后：它靠 `data-goal` 把自己和详情页分开，
    // 顺序反过来读起来会以为两者会互相抢。
    initSettings();
    // 注意：上一次记完留下的补判待办不在这里跑，它在 initComposer 的末尾——
    // 那一趟要用到输入框的状态（草稿、提示语），而那些东西的作用域在那边。
    // 每次加载报一行。用来区分「窗口起来了」和「窗口起来了但页面是空的」——
    // 这两种情况从外面看一模一样，处置却完全相反。
    // 两个行数是虚拟滚动的证据：总行数很多，真正进了 DOM 的只有可见的那几十行。
    if (T) {
      report('page', document.querySelectorAll('.card').length + ' cards, ' +
        rows.length + ' log rows, ' + (rowsEl ? rowsEl.children.length : 0) + ' in dom');
    }
  } catch (e) {
    report('启动失败', e);
  }
})();
