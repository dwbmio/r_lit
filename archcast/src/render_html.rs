//! Deterministic 1:1 HTML + inline SVG renderer matching the canonical visual specification.

use crate::backend::RenderBackend;
use crate::layout::{LaidOutBadge, LaidOutDiagram};
use crate::schema::{ArchSpec, CalloutStyle, FlowTableSpec, StyledLine};
use crate::simd_ops::escape_html_into;
use crate::theme::{ThemeConfig, default_theme, resolve_theme};
use std::fmt::Write;

const FONT_SANS: &str = "'PingFang SC','Hiragino Sans GB','Microsoft YaHei','Noto Sans CJK SC','Source Han Sans SC',sans-serif";
const FONT_MONO: &str = "'SFMono-Regular',Menlo,Consolas,'PingFang SC','Microsoft YaHei',monospace";

fn build_page_css(t: &ThemeConfig) -> String {
    format!(
        r#"  :root {{ color-scheme: {cs}; }}
  body {{ margin:0; background:{page_bg}; color:{page_fg};
         font-family:'PingFang SC','Microsoft YaHei','Noto Sans CJK SC',sans-serif; }}
  .wrap {{ max-width:1560px; margin:0 auto; padding:28px 20px 60px; }}
  .title-row {{ display:flex; align-items:center; justify-content:space-between; gap:12px; flex-wrap:wrap; margin:0 0 10px; }}
  h1 {{ font-size:25px; margin:0; }}
  .theme-chip {{ font-size:11.5px; font-weight:600; padding:3px 10px; border-radius:999px;
                 background:{pill_bg}; color:{pill_fg}; border:1px solid {card_border}; }}
  p.toc {{ font-size:13px; margin:0 0 16px; }}
  p.toc a {{ color:{toc_link}; text-decoration:none; }}
  p.toc a:hover {{ text-decoration:underline; }}
  p.lead {{ color:{lead_fg}; font-size:14px; line-height:1.8; margin:0 0 18px; }}
  .card {{ background:{card_bg}; border:1px solid {card_border}; border-radius:14px;
          padding:14px 14px 6px; margin:0 0 26px; box-shadow:{card_shadow}; }}
  .card h2 {{ font-size:17px; margin:4px 0 8px; padding-left:10px; border-left:4px solid {h2_accent}; }}
  .card p.note {{ color:{note_fg}; font-size:13px; margin:0 0 10px; padding-left:14px; line-height:1.7; }}
  svg {{ width:100%; height:auto; display:block; border-radius:8px; }}
  code {{ background:{code_bg}; color:{code_fg}; padding:1px 5px; border-radius:4px; font-size:12.5px; }}
  .card h3 {{ font-size:15px; margin:22px 0 8px; padding-left:10px; border-left:3px solid {card_border}; }}
  .card h4 {{ font-size:13.5px; margin:16px 0 6px; color:{page_fg}; }}
  .card p.hint {{ font-size:12.5px; color:{hint_fg}; background:{hint_bg}; border-left:4px solid {hint_accent};
                 padding:9px 12px; border-radius:6px; margin:8px 0 4px; line-height:1.7; }}
  table {{ width:100%; border-collapse:collapse; font-size:12.5px; margin:6px 0 10px; }}
  th {{ text-align:left; background:{table_head_bg}; color:{table_head_fg}; font-weight:600; padding:7px 10px;
       border-bottom:1px solid {table_border}; white-space:nowrap; }}
  td {{ padding:6px 10px; border-bottom:1px solid {table_row_border}; vertical-align:top; line-height:1.6; }}
  tr:hover td {{ background:{table_hover_bg}; }}
  td.mono {{ font-family:'SFMono-Regular',Menlo,Consolas,monospace; font-size:11.5px; color:{page_fg}; }}
  td.mono code {{ background:none; padding:0; color:inherit; }}
  details {{ margin-top:6px; }}
  summary {{ cursor:pointer; font-size:12.5px; color:{note_fg}; margin-bottom:6px; }}
  table.flow td {{ font-size:12px; }}
  table.flow td.small {{ color:{table_small_fg}; font-size:11.5px; white-space:nowrap; }}
  table.flow td.down {{ color:{table_down_fg}; }}
  table.flow tr.grp td {{ background:{table_grp_bg}; font-weight:700; color:{table_grp_fg}; font-size:13px;
                         padding:8px 10px; border-top:2px solid {table_border}; }}
  table.flow tr.grp .dot {{ display:inline-block; width:8px; height:8px; border-radius:50%;
                           margin-right:8px; }}
  table.flow tr.grp .cnt {{ color:{table_cnt_fg}; font-weight:400; margin-left:8px; font-size:11.5px; }}
  table.flow td .tab {{ display:block; width:4px; height:20px; border-radius:2px; }}
  table.flow .ver {{ display:inline-block; padding:1px 8px; border-radius:10px; font-size:11px;
                    font-weight:600; white-space:nowrap; }}
  table.flow tr:hover td {{ background:{table_hover_bg}; }}
  .card-head {{ display:flex; align-items:center; justify-content:space-between; gap:12px; flex-wrap:wrap; }}
  .fx-bar {{ display:flex; align-items:center; gap:6px; font-size:11.5px; }}
  .fx-btn {{ border:1px solid {btn_border}; background:{btn_bg}; color:{btn_fg}; border-radius:7px;
            padding:3px 9px; cursor:pointer; font-size:11.5px; line-height:1.5; }}
  .fx-btn:hover {{ background:{btn_hover_bg}; }}
  .fx-pill {{ padding:2px 8px; border-radius:10px; font-size:11px; font-weight:600;
             background:{pill_bg}; color:{pill_fg}; }}
  .stage {{ position:relative; }}
  .stage canvas.fx-canvas {{ position:absolute; pointer-events:none; display:block; }}
  svg g[data-card-id] {{ cursor:pointer; transition:opacity .18s ease; }}
  svg.has-focus g[data-card-id] {{ opacity:.24; }}
  svg.has-focus g[data-card-id].is-active {{ opacity:1; }}"#,
        cs = t.color_scheme,
        page_bg = t.surface.page_bg,
        page_fg = t.ink.page_fg,
        pill_bg = t.chrome.pill_bg,
        pill_fg = t.chrome.pill_fg,
        card_border = t.surface.card_border,
        toc_link = t.chrome.toc_link,
        lead_fg = t.ink.lead_fg,
        card_bg = t.surface.card_bg,
        card_shadow = t.surface.card_shadow,
        h2_accent = t.chrome.h2_accent,
        note_fg = t.ink.note_fg,
        code_bg = t.surface.code_bg,
        code_fg = t.ink.code_fg,
        hint_fg = t.warning.hint_fg,
        hint_bg = t.warning.hint_bg,
        hint_accent = t.warning.hint_accent,
        table_head_bg = t.table.head_bg,
        table_head_fg = t.table.head_fg,
        table_border = t.table.border,
        table_row_border = t.table.row_border,
        table_hover_bg = t.table.hover_bg,
        table_small_fg = t.table.small_fg,
        table_down_fg = t.table.down_fg,
        table_grp_bg = t.table.grp_bg,
        table_grp_fg = t.table.grp_fg,
        table_cnt_fg = t.table.cnt_fg,
        btn_border = t.chrome.btn_border,
        btn_bg = t.chrome.btn_bg,
        btn_fg = t.chrome.btn_fg,
        btn_hover_bg = t.chrome.btn_hover_bg,
    )
}

const MOTION_RUNTIME_JS: &str = r#"<script>
(() => {
  const PALETTE = [
    [15/255, 138/255, 95/255],
    [75/255, 131/255, 212/255],
    [224/255, 139/255, 60/255],
    [123/255, 97/255, 201/255]
  ];

  const WGSL = `
struct Uniforms { size: vec2<f32>, _pad: vec2<f32> };
struct Inst { pos: vec2<f32>, radius: f32, alpha: f32, color: vec3<f32>, _p: f32 };
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read> insts: array<Inst>;
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) col: vec3<f32>, @location(2) alpha: f32 };
@vertex fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
  var q = array<vec2<f32>, 6>(
    vec2(-1.0,-1.0), vec2(1.0,-1.0), vec2(-1.0,1.0),
    vec2(-1.0,1.0), vec2(1.0,-1.0), vec2(1.0,1.0)
  );
  let p = insts[ii];
  let uv = q[vi];
  let px = p.pos + uv * (p.radius * 2.1);
  let ndc = vec2<f32>((px.x / u.size.x) * 2.0 - 1.0, 1.0 - (px.y / u.size.y) * 2.0);
  var o: VOut;
  o.pos = vec4<f32>(ndc, 0.0, 1.0);
  o.uv = uv;
  o.col = p.color;
  o.alpha = p.alpha;
  return o;
}
@fragment fn fs(v: VOut) -> @location(0) vec4<f32> {
  let d2 = dot(v.uv, v.uv);
  if (d2 > 1.0) { discard; }
  let a = exp(-d2 * 4.2) * v.alpha;
  return vec4<f32>(v.col * a, a);
}
`;

  function buildTracks(data) {
    const pal = (data.palette && data.palette.length) ? data.palette : PALETTE;
    const tracks = [];
    (data.edges || []).forEach((e, ei) => {
      const col = pal[ei % pal.length];
      (e.polylines || []).forEach((pts, pi) => {
        if (!pts || pts.length < 2) return;
        const cum = [0];
        let acc = 0;
        for (let i = 1; i < pts.length; i++) {
          acc += Math.hypot(pts[i][0] - pts[i-1][0], pts[i][1] - pts[i-1][1]);
          cum.push(acc);
        }
        if (acc > 4) {
          tracks.push({ pts, cum, total: acc, off: ((ei * 7 + pi * 3) % 10) / 10, col, from: e.from || [], to: e.to || [] });
        }
      });
    });
    return tracks;
  }

  function sampleTrack(tr, u) {
    const target = Math.max(0, Math.min(1, u)) * tr.total;
    for (let i = 0; i < tr.pts.length - 1; i++) {
      if (target <= tr.cum[i+1] || i + 2 === tr.pts.length) {
        const seg = Math.max(tr.cum[i+1] - tr.cum[i], 1e-6);
        const t = Math.max(0, Math.min(1, (target - tr.cum[i]) / seg));
        return [
          tr.pts[i][0] + (tr.pts[i+1][0] - tr.pts[i][0]) * t,
          tr.pts[i][1] + (tr.pts[i+1][1] - tr.pts[i][1]) * t
        ];
      }
    }
    return tr.pts[tr.pts.length - 1];
  }

  async function initWebGPU(canvas) {
    if (!navigator.gpu) return null;
    try {
      const adapter = await navigator.gpu.requestAdapter();
      if (!adapter) return null;
      const device = await adapter.requestDevice();
      const ctx = canvas.getContext('webgpu');
      if (!ctx) return null;
      const fmt = navigator.gpu.getPreferredCanvasFormat();
      ctx.configure({ device, format: fmt, alphaMode: 'premultiplied' });
      const mod = device.createShaderModule({ code: WGSL });
      const pipe = device.createRenderPipeline({
        layout: 'auto',
        vertex: { module: mod, entryPoint: 'vs' },
        fragment: {
          module: mod, entryPoint: 'fs',
          targets: [{ format: fmt, blend: {
            color: { srcFactor: 'one', dstFactor: 'one-minus-src-alpha', operation: 'add' },
            alpha: { srcFactor: 'one', dstFactor: 'one-minus-src-alpha', operation: 'add' }
          }}]
        },
        primitive: { topology: 'triangle-list' }
      });
      const uBuf = device.createBuffer({ size: 16, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
      return { device, ctx, pipe, uBuf, iBuf: null, iCap: 0, bg: null };
    } catch (_) { return null; }
  }

  document.querySelectorAll('.card[data-archcast-stage]').forEach(async (card) => {
    const svg = card.querySelector('svg');
    const canvas = card.querySelector('canvas.fx-canvas');
    const dataEl = card.querySelector('script.fx-data');
    const toggleBtn = card.querySelector('[data-fx-toggle]');
    const resetBtn = card.querySelector('[data-fx-reset]');
    const dlBtn = card.querySelector('[data-fx-download]');
    const pill = card.querySelector('[data-fx-engine]');
    if (!svg || !canvas || !dataEl) return;

    const data = JSON.parse(dataEl.textContent || '{}');
    const tracks = buildTracks(data);
    let playing = true;
    let focusedCard = null;

    if (dlBtn) {
      dlBtn.addEventListener('click', () => {
        const xml = '<?xml version="1.0" encoding="UTF-8"?>\n' + svg.outerHTML;
        const blob = new Blob([xml], { type: 'image/svg+xml;charset=utf-8' });
        const a = document.createElement('a');
        a.href = URL.createObjectURL(blob);
        a.download = (card.id || 'diagram') + '.svg';
        a.click();
        setTimeout(() => URL.revokeObjectURL(a.href), 1000);
      });
    }

    function applyFocus(cid) {
      focusedCard = cid;
      if (!cid) {
        svg.classList.remove('has-focus');
        svg.querySelectorAll('g[data-card-id]').forEach(g => g.classList.remove('is-active'));
        if (resetBtn) resetBtn.hidden = true;
        return;
      }
      const active = new Set([cid]);
      tracks.forEach(tr => {
        if (tr.from.includes(cid) || tr.to.includes(cid)) {
          tr.from.forEach(x => active.add(x));
          tr.to.forEach(x => active.add(x));
        }
      });
      svg.classList.add('has-focus');
      svg.querySelectorAll('g[data-card-id]').forEach(g => {
        g.classList.toggle('is-active', active.has(g.getAttribute('data-card-id')));
      });
      if (resetBtn) resetBtn.hidden = false;
    }

    svg.addEventListener('click', (ev) => {
      const g = ev.target.closest('g[data-card-id]');
      if (!g) { applyFocus(null); return; }
      const cid = g.getAttribute('data-card-id');
      applyFocus(focusedCard === cid ? null : cid);
    });
    if (resetBtn) resetBtn.addEventListener('click', () => applyFocus(null));
    if (toggleBtn) {
      toggleBtn.addEventListener('click', () => {
        playing = !playing;
        toggleBtn.textContent = playing ? '⏸ 暂停动效' : '▶ 播放动效';
      });
    }

    const gpu = await initWebGPU(canvas);
    const ctx2d = gpu ? null : canvas.getContext('2d');
    if (pill) pill.textContent = gpu ? 'WebGPU' : 'Canvas2D';

    function resizeCanvas() {
      const rect = canvas.getBoundingClientRect();
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const cw = Math.max(1, Math.round(rect.width * dpr));
      const ch = Math.max(1, Math.round(rect.height * dpr));
      if (canvas.width !== cw || canvas.height !== ch) {
        canvas.width = cw;
        canvas.height = ch;
      }
      return { cw, ch };
    }

    const bx = (data.bounds && data.bounds.x) || 0;
    const by = (data.bounds && data.bounds.y) || 0;
    const bw = (data.bounds && data.bounds.w) || (data.width || 2100);
    const bh = (data.bounds && data.bounds.h) || (data.height || 1600);

    const startTs = performance.now();
    function tick(now) {
      const { cw, ch } = resizeCanvas();
      const sx = cw / bw;
      const sy = ch / bh;
      const phase = ((now - startTs) / 3200) % 1;

      const list = [];
      if (playing) {
        for (const tr of tracks) {
          const relevant = !focusedCard || tr.from.includes(focusedCard) || tr.to.includes(focusedCard);
          const alphaMul = relevant ? 1.0 : 0.14;
          const u = (phase + tr.off) % 1;
          for (const [lag, rm, a] of [[0, 1.0, 0.92], [0.032, 0.72, 0.52], [0.064, 0.48, 0.25]]) {
            const tu = (u - lag + 1) % 1;
            const [x, y] = sampleTrack(tr, tu);
            list.push((x - bx) * sx, (y - by) * sy, Math.max(3.2, 6.2 * sx) * rm, a * alphaMul, tr.col[0], tr.col[1], tr.col[2], 0);
          }
        }
        for (const bc of (data.beacons || [])) {
          const s = Math.sin((phase + bc.off) * Math.PI * 2) * 0.5 + 0.5;
          list.push((bc.x - bx) * sx, (bc.y - by) * sy, Math.max(3.0, 6.5 * sx) * (0.75 + 0.45 * s), 0.22 + 0.52 * s, bc.col[0], bc.col[1], bc.col[2], 0);
        }
      }

      if (gpu) {
        const count = list.length / 8;
        gpu.device.queue.writeBuffer(gpu.uBuf, 0, new Float32Array([cw, ch, 0, 0]));
        const need = Math.max(32, list.length * 4);
        if (!gpu.iBuf || need > gpu.iCap) {
          gpu.iCap = need * 2;
          gpu.iBuf = gpu.device.createBuffer({ size: gpu.iCap, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST });
          gpu.bg = gpu.device.createBindGroup({
            layout: gpu.pipe.getBindGroupLayout(0),
            entries: [
              { binding: 0, resource: { buffer: gpu.uBuf } },
              { binding: 1, resource: { buffer: gpu.iBuf } }
            ]
          });
        }
        if (count > 0) gpu.device.queue.writeBuffer(gpu.iBuf, 0, new Float32Array(list));
        const enc = gpu.device.createCommandEncoder();
        const pass = enc.beginRenderPass({
          colorAttachments: [{
            view: gpu.ctx.getCurrentTexture().createView(),
            clearValue: { r: 0, g: 0, b: 0, a: 0 },
            loadOp: 'clear',
            storeOp: 'store'
          }]
        });
        if (count > 0) {
          pass.setPipeline(gpu.pipe);
          pass.setBindGroup(0, gpu.bg);
          pass.draw(6, count, 0, 0);
        }
        pass.end();
        gpu.device.queue.submit([enc.finish()]);
      } else if (ctx2d) {
        ctx2d.clearRect(0, 0, cw, ch);
        for (let i = 0; i < list.length; i += 8) {
          const x = list[i], y = list[i+1], r = list[i+2], a = list[i+3];
          const cr = Math.round(list[i+4]*255), cg = Math.round(list[i+5]*255), cb = Math.round(list[i+6]*255);
          const g = ctx2d.createRadialGradient(x, y, 0, x, y, r * 2.1);
          g.addColorStop(0, `rgba(${cr},${cg},${cb},${a.toFixed(3)})`);
          g.addColorStop(0.5, `rgba(${cr},${cg},${cb},${(a*0.35).toFixed(3)})`);
          g.addColorStop(1, `rgba(${cr},${cg},${cb},0)`);
          ctx2d.fillStyle = g;
          ctx2d.beginPath();
          ctx2d.arc(x, y, r * 2.1, 0, Math.PI * 2);
          ctx2d.fill();
        }
      }
      requestAnimationFrame(tick);
    }
    requestAnimationFrame(tick);
  });
})();
</script>"#;

/// Wrap a rendered `<svg>...</svg>` body into a standalone XML SVG file.
pub fn render_standalone_svg(svg_body: &str) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{svg_body}\n")
}

/// Render a complete standalone HTML+SVG document using the chosen backend and spec theme (or Starlink Light default).
pub fn render_html_document(spec: &ArchSpec, backend: &dyn RenderBackend) -> String {
    let theme = resolve_theme(spec.theme.as_deref(), None).unwrap_or_else(|_| default_theme());
    let rendered = backend.render_diagrams(&spec.diagrams, &theme);
    render_html_from_rendered(spec, &rendered, true, &theme)
}

/// Render a complete standalone HTML+SVG document from pre-rendered diagrams with the specified global theme.
pub fn render_html_from_rendered(
    spec: &ArchSpec,
    rendered_diagrams: &[(LaidOutDiagram, String)],
    motion: bool,
    theme: &ThemeConfig,
) -> String {
    let mut out = String::with_capacity(32_768);
    out.push_str("<!doctype html>\n<html lang=\"zh-CN\" data-archcast-theme=\"");
    escape_html_into(&theme.id, &mut out);
    out.push_str("\">\n<head>\n<meta charset=\"utf-8\">\n<title>");
    escape_html_into(
        spec.doc_title.as_deref().unwrap_or(&spec.title),
        &mut out,
    );
    out.push_str("</title>\n<style>\n");
    out.push_str(&build_page_css(theme));
    out.push_str("\n</style>\n</head>\n<body><div class=\"wrap\">\n<div class=\"title-row\"><h1>");
    escape_html_into(&spec.title, &mut out);
    out.push_str("</h1><span class=\"theme-chip\">");
    escape_html_into(&theme.display_name, &mut out);
    out.push_str("</span></div>\n");

    // Table of contents anchors.
    let mut toc_items: Vec<(&str, &str)> = rendered_diagrams
        .iter()
        .map(|(d, _)| (d.id.as_str(), d.heading.as_str()))
        .collect();
    if let Some(ft) = &spec.flow_table {
        toc_items.push((ft.id.as_str(), ft.heading.as_str()));
    }
    if !toc_items.is_empty() {
        out.push_str("<p class=\"toc\">");
        for (i, (id, heading)) in toc_items.iter().enumerate() {
            if i > 0 {
                out.push_str(" · ");
            }
            out.push_str("<a href=\"#");
            escape_html_into(id, &mut out);
            out.push_str("\">");
            escape_html_into(heading, &mut out);
            out.push_str("</a>");
        }
        out.push_str("</p>\n");
    }

    // Lead summary lines with inline `code` and **bold** formatting.
    if !spec.lead.is_empty() {
        out.push_str("<p class=\"lead\">\n");
        for (i, line) in spec.lead.iter().enumerate() {
            if i > 0 {
                out.push_str("<br>\n");
            }
            write_inline_rich_html(line, &mut out);
        }
        out.push_str("\n</p>\n");
    }

    // Diagram cards.
    for (diag, svg) in rendered_diagrams {
        if motion {
            out.push_str("<div class=\"card\" id=\"");
            escape_html_into(&diag.id, &mut out);
            out.push_str("\" data-archcast-stage=\"1\">\n  <div class=\"card-head\"><h2>");
            escape_html_into(&diag.heading, &mut out);
            out.push_str("</h2><div class=\"fx-bar\"><span class=\"fx-pill\" data-fx-engine>Motion</span><button type=\"button\" class=\"fx-btn\" data-fx-toggle>⏸ 暂停动效</button><button type=\"button\" class=\"fx-btn\" data-fx-reset hidden>重置高亮</button><button type=\"button\" class=\"fx-btn\" data-fx-download>⬇ SVG</button></div></div>\n");
        } else {
            out.push_str("<div class=\"card\" id=\"");
            escape_html_into(&diag.id, &mut out);
            out.push_str("\">\n  <h2>");
            escape_html_into(&diag.heading, &mut out);
            out.push_str("</h2>\n");
        }
        if !diag.note.is_empty() {
            out.push_str("  <p class=\"note\">");
            write_inline_rich_html(&diag.note, &mut out);
            out.push_str("</p>\n");
        }
        if motion {
            let bounds = diag.dynamic_bounds(24.0);
            out.push_str("  <div class=\"stage\">\n  ");
            out.push_str(svg);
            if let Some(b) = bounds {
                let left_pct = (b.x / diag.width) * 100.0;
                let top_pct = (b.y / diag.height) * 100.0;
                let w_pct = (b.w / diag.width) * 100.0;
                let h_pct = (b.h / diag.height) * 100.0;
                let _ = write!(
                    out,
                    "\n  <canvas class=\"fx-canvas\" style=\"left:{left_pct:.3}%;top:{top_pct:.3}%;width:{w_pct:.3}%;height:{h_pct:.3}%\" aria-hidden=\"true\"></canvas>\n  </div>\n  <script type=\"application/json\" class=\"fx-data\">"
                );
                out.push_str(&build_motion_json(diag, b, theme));
                out.push_str("</script>\n</div>\n\n");
            } else {
                out.push_str("\n  </div>\n</div>\n\n");
            }
        } else {
            out.push_str("  ");
            out.push_str(svg);
            out.push_str("\n</div>\n\n");
        }
    }

    // Section 3: Flow table.
    if let Some(table) = &spec.flow_table {
        render_flow_table(table, theme, &mut out);
    }

    out.push_str("</div>");
    if motion && !rendered_diagrams.is_empty() {
        out.push('\n');
        out.push_str(MOTION_RUNTIME_JS);
    }
    out.push_str("</body></html>\n");
    out
}

fn build_motion_json(
    d: &LaidOutDiagram,
    bounds: crate::layout::DynamicBounds,
    theme: &ThemeConfig,
) -> String {
    let edges: Vec<serde_json::Value> = d
        .edges
        .iter()
        .map(|e| {
            serde_json::json!({
                "from": e.from_ids,
                "to": e.to_ids,
                "polylines": e.polylines,
            })
        })
        .collect();

    let red_rgb = theme.beacon_red_rgb();
    let green_rgb = theme.beacon_green_rgb();
    let mut beacons = Vec::new();
    let mut bi = 0usize;
    for col in &d.columns {
        for card in &col.cards {
            if let Some(b) = &card.badge
                && matches!(
                    b.spec.tone,
                    crate::schema::Tone::Red
                        | crate::schema::Tone::Emerald
                        | crate::schema::Tone::Green
                )
            {
                let rgb = match b.spec.tone {
                    crate::schema::Tone::Red => red_rgb,
                    _ => green_rgb,
                };
                beacons.push(serde_json::json!({
                    "x": b.rect.x,
                    "y": b.rect.cy(),
                    "off": (bi % 6) as f64 / 6.0,
                    "col": [rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0],
                }));
                bi += 1;
            }
        }
    }

    let palette: Vec<[f32; 3]> = theme
        .pulse_rgb()
        .iter()
        .map(|rgb| [rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0])
        .collect();

    let payload = serde_json::json!({
        "width": d.width,
        "height": d.height,
        "bounds": bounds,
        "palette": palette,
        "edges": edges,
        "beacons": beacons,
    });
    // Prevent `</script>` inside embedded JSON.
    payload.to_string().replace("</", "<\\/")
}

/// Emit one deterministic `<svg>...</svg>` element using the default Starlink Light theme.
pub fn render_diagram_svg(d: &LaidOutDiagram) -> String {
    let theme = default_theme();
    render_diagram_svg_with_theme(d, &theme)
}

/// Emit one deterministic `<svg>...</svg>` element styled with `theme`.
pub fn render_diagram_svg_with_theme(d: &LaidOutDiagram, t: &ThemeConfig) -> String {
    let mut s = String::with_capacity(24_576);
    let marker_id = format!("ar-{}", sanitize_id(&d.id));

    let _ = writeln!(
        s,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.0}\" height=\"{:.0}\" viewBox=\"0 0 {:.0} {:.0}\" font-family=\"{FONT_SANS}\" role=\"img\">",
        d.width, d.height, d.width, d.height
    );
    s.push_str("<title>");
    escape_html_into(&d.svg_title, &mut s);
    s.push_str("</title>\n");

    let _ = writeln!(
        s,
        "<defs><marker id=\"{marker_id}\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\"><path d=\"M0,0 L10,5 L0,10 z\" fill=\"{}\"/></marker></defs>",
        t.chrome.edge_stroke
    );
    let _ = writeln!(
        s,
        "<rect x=\"0\" y=\"0\" width=\"{:.0}\" height=\"{:.0}\" fill=\"{}\"/>",
        d.width, d.height, t.surface.svg_bg
    );

    // Canvas title & subtitles.
    let _ = write!(
        s,
        "<text x=\"50\" y=\"54\" font-size=\"27\" fill=\"{}\" font-weight=\"bold\" text-anchor=\"start\">",
        t.ink.svg_title
    );
    escape_html_into(&d.canvas_title, &mut s);
    s.push_str("</text>\n");

    for (i, (sy, line)) in d.subtitles.iter().enumerate() {
        let size = line.size.unwrap_or(if i == 0 { 13.5 } else { 11.5 });
        let fill: &str = if line.tone.is_default() {
            if i == 0 {
                &t.ink.svg_subtitle
            } else {
                &t.ink.svg_meta
            }
        } else {
            t.tone_hex(line.tone)
        };
        let weight = if line.bold { "bold" } else { "normal" };
        let _ = write!(
            s,
            "<text x=\"50\" y=\"{sy:.1}\" font-size=\"{size:.1}\" fill=\"{fill}\" font-weight=\"{weight}\" text-anchor=\"start\">"
        );
        escape_html_into(&line.text, &mut s);
        s.push_str("</text>\n");
    }

    // Columns & cards.
    for col in &d.columns {
        let _ = write!(
            s,
            "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"12.5\" fill=\"{}\" font-weight=\"bold\" text-anchor=\"start\">",
            col.header_x, col.header_y, t.ink.col_header
        );
        escape_html_into(&col.title, &mut s);
        s.push_str("</text>\n");

        for card in &col.cards {
            let r = &card.rect;
            let accent_hex = t.tone_hex(card.spec.accent);
            let _ = writeln!(
                s,
                "<g data-card-id=\"{}\">",
                crate::simd_ops::escape_html(&card.spec.id)
            );
            let _ = writeln!(
                s,
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"10\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.2\"/>",
                r.x, r.y, r.w, r.h, t.surface.node_bg, t.surface.node_border
            );
            let _ = writeln!(
                s,
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"6\" height=\"{:.1}\" fill=\"{accent_hex}\" rx=\"2\"/>",
                r.x, r.y, r.h
            );
            let _ = write!(
                s,
                "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"15.5\" fill=\"{}\" font-weight=\"bold\" text-anchor=\"start\">",
                r.x + 18.0,
                r.y + 26.0,
                t.ink.node_title
            );
            escape_html_into(&card.spec.title, &mut s);
            s.push_str("</text>\n");

            if let Some(badge) = &card.badge {
                write_badge_svg(badge, t, &mut s);
            }

            // Card lines.
            let mut ly = r.y + 52.0;
            for line in &card.spec.lines {
                write_styled_svg_line(r.x + 18.0, ly, line, 11.5, 10.4, t, &mut s);
                ly += if line.mono { 20.5 } else { 22.0 };
            }

            // Nested action subcards.
            for sub in &card.subcards {
                let sr = &sub.rect;
                let _ = writeln!(
                    s,
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"8\" fill=\"{}\" stroke=\"{}\"/>",
                    sr.x, sr.y, sr.w, sr.h, t.surface.subcard_bg, t.surface.subcard_border
                );
                let _ = writeln!(
                    s,
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"4\" height=\"{:.1}\" fill=\"{accent_hex}\" rx=\"2\"/>",
                    sr.x, sr.y, sr.h
                );
                let _ = write!(
                    s,
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"12.5\" fill=\"{}\" font-weight=\"bold\" text-anchor=\"start\">",
                    sr.x + 14.0,
                    sr.y + 22.0,
                    t.ink.subcard_title
                );
                escape_html_into(&sub.spec.title, &mut s);
                s.push_str("</text>\n");

                if let Some(sb) = &sub.badge {
                    write_badge_svg(sb, t, &mut s);
                }

                let mut sly = sr.y + 42.0;
                for (li, line) in sub.spec.lines.iter().enumerate() {
                    let mono_size = if li == 0 { 10.2 } else { 9.5 };
                    write_styled_svg_line(sr.x + 14.0, sly, line, 11.0, mono_size, t, &mut s);
                    sly += 15.5;
                }
            }

            // Optional footer note inside card.
            if let Some(footer) = &card.spec.footer {
                let fy = r.bottom() - 10.0;
                let _ = write!(
                    s,
                    "<text x=\"{:.1}\" y=\"{fy:.1}\" font-size=\"10.5\" fill=\"{}\" font-weight=\"normal\" text-anchor=\"start\">",
                    r.x + 18.0,
                    t.ink.node_footer
                );
                escape_html_into(footer, &mut s);
                s.push_str("</text>\n");
            }
            s.push_str("</g>\n");
        }
    }

    // Routed edges & bus labels.
    for edge in &d.edges {
        for (x1, y1, x2, y2) in &edge.spines {
            let _ = writeln!(
                s,
                "<line x1=\"{x1:.1}\" y1=\"{y1:.1}\" x2=\"{x2:.1}\" y2=\"{y2:.1}\" stroke=\"{}\" stroke-width=\"1.8\"/>",
                t.chrome.edge_spine
            );
        }
        for path in &edge.paths {
            let _ = writeln!(
                s,
                "<path d=\"{path}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.7\" stroke-linejoin=\"round\" marker-end=\"url(#{marker_id})\"/>",
                t.chrome.edge_stroke
            );
        }
        if let Some((lx, ly, lw, text)) = &edge.label {
            let _ = writeln!(
                s,
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{lw:.1}\" height=\"18\" fill=\"{}\"/>",
                lx - lw * 0.5,
                ly - 13.0,
                t.chrome.edge_label_bg
            );
            let _ = write!(
                s,
                "<text x=\"{lx:.1}\" y=\"{ly:.1}\" font-size=\"11\" fill=\"{}\" font-weight=\"bold\" text-anchor=\"middle\">",
                t.chrome.edge_label_fg
            );
            escape_html_into(text, &mut s);
            s.push_str("</text>\n");
        }
    }

    // Bottom callout cards.
    for callout in &d.callouts {
        let r = &callout.rect;
        let accent = t.tone_hex(callout.spec.accent);
        let _ = writeln!(
            s,
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"12\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.2\"/>",
            r.x, r.y, r.w, r.h, t.surface.callout_bg, t.surface.callout_border
        );
        let _ = writeln!(
            s,
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"6\" height=\"{:.1}\" fill=\"{accent}\" rx=\"3\"/>",
            r.x, r.y, r.h
        );
        let _ = write!(
            s,
            "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"14.5\" fill=\"{}\" font-weight=\"bold\" text-anchor=\"start\">",
            r.x + 20.0,
            r.y + 27.0,
            t.ink.node_title
        );
        escape_html_into(&callout.spec.title, &mut s);
        s.push_str("</text>\n");

        match callout.spec.style {
            CalloutStyle::CodeBox => {
                let box_h = (r.h - 60.0).max(60.0);
                let _ = writeln!(
                    s,
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{box_h:.1}\" rx=\"8\" fill=\"{}\" stroke=\"{}\"/>",
                    r.x + 20.0,
                    r.y + 42.0,
                    r.w - 40.0,
                    t.surface.codebox_bg,
                    t.surface.codebox_border
                );
                let mut ly = r.y + 62.0;
                for line in &callout.spec.lines {
                    let mut mono_line = line.clone();
                    mono_line.mono = true;
                    write_styled_svg_line(r.x + 32.0, ly, &mono_line, 10.4, 10.4, t, &mut s);
                    ly += 17.5;
                }
            }
            CalloutStyle::Bullets => {
                let mut ly = r.y + 54.0;
                for line in &callout.spec.lines {
                    write_styled_svg_line(r.x + 20.0, ly, line, 12.5, 11.0, t, &mut s);
                    ly += 21.5;
                }
            }
        }
    }

    // Optional full-width warning banner.
    if let Some(warn) = &d.warning {
        let r = &warn.rect;
        let _ = writeln!(
            s,
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"10\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.4\"/>",
            r.x, r.y, r.w, r.h, t.warning.banner_bg, t.warning.banner_border
        );
        let _ = writeln!(
            s,
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"6\" height=\"{:.1}\" fill=\"{}\" rx=\"3\"/>",
            r.x, r.y, r.h, t.warning.banner_accent
        );
        let _ = write!(
            s,
            "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"14.5\" fill=\"{}\" font-weight=\"bold\" text-anchor=\"start\">",
            r.x + 24.0,
            r.y + 28.0,
            t.warning.banner_title
        );
        escape_html_into(&warn.spec.title, &mut s);
        s.push_str("</text>\n");

        let _ = write!(
            s,
            "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"12.5\" fill=\"{}\" font-weight=\"normal\" text-anchor=\"start\">",
            r.x + 24.0,
            r.y + 52.0,
            t.warning.banner_body
        );
        escape_html_into(&warn.spec.body, &mut s);
        s.push_str("</text>\n");
    }

    // Source basis footer.
    if !d.footer.is_empty() {
        let _ = write!(
            s,
            "<text x=\"50\" y=\"{:.1}\" font-size=\"11\" fill=\"{}\" font-weight=\"normal\" text-anchor=\"start\">",
            d.footer_y, t.ink.svg_meta
        );
        escape_html_into(&d.footer, &mut s);
        s.push_str("</text>\n");
    }

    s.push_str("</svg>");
    s
}

fn write_badge_svg(badge: &LaidOutBadge, t: &ThemeConfig, s: &mut String) {
    let r = &badge.rect;
    let hex = t.tone_hex(badge.spec.tone);
    let _ = writeln!(
        s,
        "<rect x=\"{:.2}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"10\" fill=\"{hex}\" opacity=\"{}\"/>",
        r.x, r.y, r.w, r.h, t.chrome.badge_opacity
    );
    let _ = write!(
        s,
        "<text x=\"{:.2}\" y=\"{:.1}\" font-size=\"10.5\" fill=\"{hex}\" font-weight=\"bold\" text-anchor=\"middle\">",
        r.cx(),
        r.y + 14.0
    );
    escape_html_into(&badge.spec.text, s);
    s.push_str("</text>\n");
}

fn write_styled_svg_line(
    x: f64,
    y: f64,
    line: &StyledLine,
    default_size: f64,
    mono_size: f64,
    t: &ThemeConfig,
    s: &mut String,
) {
    let size = line.size.unwrap_or(if line.mono { mono_size } else { default_size });
    let fill: &str = if line.tone.is_default() {
        if line.mono {
            &t.ink.node_mono
        } else {
            &t.ink.node_text
        }
    } else {
        t.tone_hex(line.tone)
    };
    let weight = if line.bold { "bold" } else { "normal" };
    if line.mono {
        let _ = write!(
            s,
            "<text x=\"{x:.1}\" y=\"{y:.1}\" font-size=\"{size:.1}\" fill=\"{fill}\" font-weight=\"{weight}\" text-anchor=\"start\" font-family=\"{FONT_MONO}\">"
        );
    } else {
        let _ = write!(
            s,
            "<text x=\"{x:.1}\" y=\"{y:.1}\" font-size=\"{size:.1}\" fill=\"{fill}\" font-weight=\"{weight}\" text-anchor=\"start\">"
        );
    }
    escape_html_into(&line.text, s);
    s.push_str("</text>\n");
}

fn render_flow_table(table: &FlowTableSpec, t: &ThemeConfig, out: &mut String) {
    out.push_str("<div class=\"card\" id=\"");
    escape_html_into(&table.id, out);
    out.push_str("\">\n  <h2>");
    escape_html_into(&table.heading, out);
    out.push_str("</h2>\n");
    if !table.note.is_empty() {
        out.push_str("  <p class=\"note\">");
        write_inline_rich_html(&table.note, out);
        out.push_str("</p>\n");
    }
    out.push_str("  <table class='flow'><thead><tr><th>链路</th><th>入口 / 动作</th><th>触发方</th><th>版本</th><th>① 核心入口</th><th>② 下游调用（服务 · 接口）</th><th>说明</th></tr></thead><tbody>\n");

    for grp in &table.groups {
        let ghex = t.tone_hex(grp.tone);
        let _ = write!(
            out,
            "<tr class='grp'><td colspan='7'><span class='dot' style='background:{ghex}'></span>"
        );
        escape_html_into(&grp.name, out);
        let _ = writeln!(out, "<span class='cnt'>{}</span></td></tr>", grp.rows.len());

        for row in &grp.rows {
            let vhex = t.tone_hex(row.version_tone);
            let _ = write!(
                out,
                "<tr><td><span class='tab' style='background:{ghex}'></span></td><td><b>"
            );
            escape_html_into(&row.action, out);
            out.push_str("</b></td><td class='small'>");
            escape_html_into(&row.trigger, out);
            let _ = write!(
                out,
                "</td><td><span class='ver' style='color:{vhex};background:{vhex}{}'>",
                t.chrome.ver_bg_alpha
            );
            escape_html_into(&row.version, out);
            out.push_str("</span></td><td class='mono'>");
            for (ei, ep) in row.entry.iter().enumerate() {
                if ei > 0 {
                    out.push_str("<br>");
                }
                out.push_str("<code>");
                escape_html_into(ep, out);
                out.push_str("</code>");
            }
            out.push_str("</td><td class='mono down'>");
            for (di, dn) in row.downstream.iter().enumerate() {
                if di > 0 {
                    out.push_str("<br>");
                }
                escape_html_into(dn, out);
            }
            out.push_str("</td><td class='small'>");
            write_inline_rich_html(&row.note, out);
            out.push_str("</td></tr>\n");
        }
    }

    out.push_str("  </tbody></table>\n");
    if let Some(hint) = &table.hint {
        out.push_str("  <p class=\"hint\">");
        write_inline_rich_html(hint, out);
        out.push_str("</p>\n");
    }
    out.push_str("</div>\n");
}

/// Convert minimal inline markdown (`code` and **bold**) to safe HTML.
fn write_inline_rich_html(input: &str, out: &mut String) {
    let chars: Vec<char> = input.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut plain = String::new();

    while i < n {
        if chars[i] == '`'
            && let Some(end) = chars[i + 1..].iter().position(|&c| c == '`')
        {
            if !plain.is_empty() {
                escape_html_into(&plain, out);
                plain.clear();
            }
            let inner: String = chars[i + 1..i + 1 + end].iter().collect();
            out.push_str("<code>");
            escape_html_into(&inner, out);
            out.push_str("</code>");
            i += end + 2;
            continue;
        }
        if i + 1 < n && chars[i] == '*' && chars[i + 1] == '*' {
            let rest = &chars[i + 2..];
            let mut end_opt = None;
            for j in 0..rest.len().saturating_sub(1) {
                if rest[j] == '*' && rest[j + 1] == '*' {
                    end_opt = Some(j);
                    break;
                }
            }
            if let Some(end) = end_opt {
                if !plain.is_empty() {
                    escape_html_into(&plain, out);
                    plain.clear();
                }
                let inner: String = rest[..end].iter().collect();
                out.push_str("<b>");
                escape_html_into(&inner, out);
                out.push_str("</b>");
                i += end + 4;
                continue;
            }
        }
        plain.push(chars[i]);
        i += 1;
    }
    if !plain.is_empty() {
        escape_html_into(&plain, out);
    }
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}
