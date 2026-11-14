(cfg) => {
  // Bundled CDP payload. Runs inside the explicitly bound official portal tab.
  // Never reads cookies, passwords, page instructions, or hidden input values.
  if (location.protocol !== "https:" || !cfg.hosts.includes(location.hostname)
      || (location.port && location.port !== "443")) {
    return {ok: false, code: "origin"};
  }
  const clean = (text) => (text || "").replace(/[\s*＊:：]/g, "").toLowerCase();
  const writable = (el) => el && (el.tagName === "TEXTAREA"
    || (el.tagName === "INPUT" && ["text", "search"].includes(el.type)))
    && !el.disabled && !el.readOnly && el.getClientRects().length > 0;
  const fields = Array.from(document.querySelectorAll("input,textarea")).filter(writable);
  const labels = (el) => [
    ...Array.from(el.labels || []).map((label) => label.textContent),
    el.getAttribute("aria-label"), el.getAttribute("placeholder"), el.getAttribute("name"),
    el.closest(".ant-form-item,.el-form-item,.form-item")
      ?.querySelector("label,[class*='label']")?.textContent,
  ].filter(Boolean).map(clean);
  const definitions = [
    {key: "name", value: cfg.name, aliases: ["小程序名称", "小游戏名称", "游戏名称", "应用名称", "appname", "小程序名称（必填）"]},
    {key: "summary", value: cfg.summary, aliases: ["小程序简介", "小游戏简介", "应用简介", "简介", "小程序介绍", "description", "summary"]},
  ];
  const plan = [];
  const skipped = [];
  for (const def of definitions) {
    if (!def.value) { skipped.push(def.key); continue; }
    let candidates;
    const selector = cfg.selectors[def.key];
    if (selector) {
      try { candidates = Array.from(document.querySelectorAll(selector)); }
      catch (_) { return {ok: false, code: "invalid_selector"}; }
      if (candidates.some((el) => !writable(el))) return {ok: false, code: "invalid_selector"};
    } else {
      const names = def.aliases.map(clean);
      candidates = fields.filter((el) => labels(el).some((label) => names.includes(label)));
    }
    if (candidates.length > 1) return {ok: false, code: "ambiguous_" + def.key};
    if (candidates.length === 0) {
      if (def.key === "name" || selector) return {ok: false, code: "missing_" + def.key};
      skipped.push(def.key);
      continue;
    }
    const el = candidates[0];
    if (el.value && el.value !== def.value) return {ok: false, code: "occupied_" + def.key};
    if (el.maxLength >= 0 && def.value.length > el.maxLength) return {ok: false, code: "maxlength"};
    plan.push({el, def});
  }
  if (new Set(plan.map((entry) => entry.el)).size !== plan.length) {
    return {ok: false, code: "invalid_selector"};
  }
  // Validate the entire plan before the first write. Never click submit or consent.
  if (cfg.apply) {
    for (const {el, def} of plan) {
      const proto = el.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
      if (!setter) return {ok: false, code: "write_failed"};
      setter.call(el, def.value);
      el.dispatchEvent(new Event("input", {bubbles: true}));
      el.dispatchEvent(new Event("change", {bubbles: true}));
      if (el.value !== def.value) return {ok: false, code: "write_failed"};
    }
  }
  return {ok: true, applied: cfg.apply, fields: plan.map(({def}) => def.key),
    skipped, submitted: false, human_required: ["app_kind", "category", "identity", "consent", "submit"]};
}
