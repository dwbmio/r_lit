use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Render CommonMark + GFM Markdown (headings, bold/italic/strikethrough, inline code,
/// links, nested ordered/unordered/task lists, fenced code blocks, blockquotes, tables,
/// horizontal rules) into cleanly wrapped, light/dark-terminal-adaptive Ratatui lines.
pub(crate) fn render_markdown(md: &str, max_w: usize, out: &mut Vec<Line<'static>>) {
    let max_w = max_w.max(24);
    let normalized = normalize_cjk_bold(md);
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(&normalized, opts);
    let mut renderer = MdRenderer::new(max_w, out);
    for ev in parser {
        renderer.handle_event(ev);
    }
    renderer.finish();
}

/// Work around CommonMark's flanking-delimiter quirk where Chinese bold spans like
/// `**版本校验（事务外）**导致问题` fail to close because `**` sits between CJK punctuation
/// and a CJK word character. Leaves code fences and inline code untouched.
fn normalize_cjk_bold(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + 16);
    let mut in_fence = false;

    for (line_idx, line) in md.split('\n').enumerate() {
        if line_idx > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence || !line.contains("**") {
            out.push_str(line);
            continue;
        }

        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        let mut in_code = false;
        let mut bold_open = false;

        while i < chars.len() {
            if chars[i] == '`' {
                in_code = !in_code;
                out.push('`');
                i += 1;
                continue;
            }
            if !in_code && i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
                let prev = if i > 0 { Some(chars[i - 1]) } else { None };
                let next = chars.get(i + 2).copied();
                if !bold_open {
                    if prev.is_some_and(is_cjk_or_word) && next.is_some_and(is_cjk_punct) {
                        out.push(' ');
                    }
                    out.push_str("**");
                    bold_open = true;
                } else {
                    out.push_str("**");
                    if prev.is_some_and(is_cjk_punct) && next.is_some_and(is_cjk_or_word) {
                        out.push(' ');
                    }
                    bold_open = false;
                }
                i += 2;
                continue;
            }
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn is_cjk_punct(c: char) -> bool {
    matches!(
        c,
        '（' | '）'
            | '「'
            | '」'
            | '『'
            | '』'
            | '【'
            | '】'
            | '《'
            | '》'
            | '“'
            | '”'
            | '‘'
            | '’'
            | '。'
            | '！'
            | '？'
            | '；'
            | '：'
            | '、'
            | '，'
            | '('
            | ')'
            | '['
            | ']'
    )
}

fn is_cjk_or_word(c: char) -> bool {
    !c.is_whitespace() && !is_cjk_punct(c) && !c.is_ascii_punctuation()
}

#[derive(Debug, Clone)]
struct ListState {
    ordered: bool,
    next_num: u64,
}

struct TableState {
    alignments: Vec<Alignment>,
    header: Vec<Vec<Span<'static>>>,
    rows: Vec<Vec<Vec<Span<'static>>>>,
    current_row: Vec<Vec<Span<'static>>>,
    in_head: bool,
}

struct MdRenderer<'a> {
    max_w: usize,
    out: &'a mut Vec<Line<'static>>,
    style_stack: Vec<Style>,
    buf: Vec<Span<'static>>,
    heading_level: Option<HeadingLevel>,
    lists: Vec<ListState>,
    pending_list_marker: Option<(String, Style, usize)>,
    item_emitted_first_line: bool,
    blockquote_depth: usize,
    in_code_block: bool,
    code_lang: String,
    code_buf: String,
    link_dest: Option<String>,
    table: Option<TableState>,
    needs_blank_before_block: bool,
}

impl<'a> MdRenderer<'a> {
    fn new(max_w: usize, out: &'a mut Vec<Line<'static>>) -> Self {
        Self {
            max_w,
            out,
            style_stack: vec![Style::default().fg(Color::Reset)],
            buf: Vec::new(),
            heading_level: None,
            lists: Vec::new(),
            pending_list_marker: None,
            item_emitted_first_line: false,
            blockquote_depth: 0,
            in_code_block: false,
            code_lang: String::new(),
            code_buf: String::new(),
            link_dest: None,
            table: None,
            needs_blank_before_block: false,
        }
    }

    fn current_style(&self) -> Style {
        self.style_stack
            .last()
            .copied()
            .unwrap_or_else(|| Style::default().fg(Color::Reset))
    }

    fn push_style_mod(&mut self, f: impl FnOnce(Style) -> Style) {
        let next = f(self.current_style());
        self.style_stack.push(next);
    }

    fn pop_style(&mut self) {
        if self.style_stack.len() > 1 {
            self.style_stack.pop();
        }
    }

    fn emit_blank_if_needed(&mut self) {
        if self.needs_blank_before_block {
            if self.out.last().is_some_and(|l| !l.spans.is_empty()) {
                self.out.push(Line::raw(""));
            }
            self.needs_blank_before_block = false;
        }
    }

    fn handle_event(&mut self, ev: Event<'_>) {
        match ev {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => {
                if self.in_code_block {
                    self.code_buf.push_str(&text);
                } else {
                    let st = self.current_style();
                    push_merged_span(&mut self.buf, text.as_ref(), st);
                }
            }
            Event::Code(code) => {
                let code_style = Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD);
                push_merged_span(&mut self.buf, code.as_ref(), code_style);
            }
            Event::SoftBreak => {
                if self.in_code_block {
                    self.code_buf.push('\n');
                } else {
                    let need_space = self
                        .buf
                        .last()
                        .and_then(|s| s.content.chars().last())
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == ',' || c == '.');
                    if need_space {
                        let st = self.current_style();
                        push_merged_span(&mut self.buf, " ", st);
                    }
                }
            }
            Event::HardBreak => {
                if self.in_code_block {
                    self.code_buf.push('\n');
                } else if self.table.is_some() {
                    let st = self.current_style();
                    push_merged_span(&mut self.buf, " / ", st);
                } else {
                    self.flush_inline_buffer(false);
                }
            }
            Event::Rule => {
                self.flush_inline_buffer(false);
                self.emit_blank_if_needed();
                let rule_w = self.max_w.saturating_sub(4).clamp(12, 64);
                self.out.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("─".repeat(rule_w), Style::default().fg(Color::DarkGray)),
                ]));
                self.needs_blank_before_block = true;
            }
            Event::TaskListMarker(checked) => {
                let (sym, color) = if checked {
                    ("☑ ", Color::Green)
                } else {
                    ("☐ ", Color::DarkGray)
                };
                self.buf.push(Span::styled(
                    sym.to_string(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ));
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                if self.in_code_block {
                    self.code_buf.push_str(&html);
                } else {
                    let t = html.trim();
                    if t.eq_ignore_ascii_case("<br>")
                        || t.eq_ignore_ascii_case("<br/>")
                        || t.eq_ignore_ascii_case("<br />")
                    {
                        self.flush_inline_buffer(false);
                    } else if !t.is_empty() {
                        push_merged_span(&mut self.buf, t, Style::default().fg(Color::DarkGray));
                    }
                }
            }
            Event::FootnoteReference(name) => {
                push_merged_span(
                    &mut self.buf,
                    &format!("[^{name}]"),
                    Style::default().fg(Color::Cyan),
                );
            }
            _ => {}
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                if self.lists.is_empty() {
                    self.emit_blank_if_needed();
                } else if self.item_emitted_first_line && !self.buf.is_empty() {
                    self.flush_inline_buffer(false);
                }
            }
            Tag::Heading { level, .. } => {
                self.flush_inline_buffer(false);
                self.emit_blank_if_needed();
                self.heading_level = Some(level);
                let style = heading_style(level);
                self.style_stack.push(style);
            }
            Tag::BlockQuote(_) => {
                self.flush_inline_buffer(false);
                if self.blockquote_depth == 0 {
                    self.emit_blank_if_needed();
                }
                self.blockquote_depth += 1;
                self.push_style_mod(|s| s.fg(Color::DarkGray).add_modifier(Modifier::ITALIC));
            }
            Tag::CodeBlock(kind) => {
                self.flush_inline_buffer(false);
                self.emit_blank_if_needed();
                self.in_code_block = true;
                self.code_buf.clear();
                self.code_lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
            }
            Tag::List(start_num) => {
                if !self.buf.is_empty() {
                    self.flush_inline_buffer(false);
                }
                if self.lists.is_empty() {
                    self.emit_blank_if_needed();
                }
                self.lists.push(ListState {
                    ordered: start_num.is_some(),
                    next_num: start_num.unwrap_or(1),
                });
            }
            Tag::Item => {
                if !self.buf.is_empty() {
                    self.flush_inline_buffer(false);
                }
                let depth = self.lists.len().saturating_sub(1);
                let (marker, style) = if let Some(top) = self.lists.last_mut() {
                    if top.ordered {
                        let n = top.next_num;
                        top.next_num += 1;
                        (
                            format!("{n}. "),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        let bullet = match depth {
                            0 => "• ",
                            1 => "◦ ",
                            _ => "▪ ",
                        };
                        (
                            bullet.to_string(),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        )
                    }
                } else {
                    ("• ".to_string(), Style::default().fg(Color::Cyan))
                };
                self.pending_list_marker = Some((marker, style, depth));
                self.item_emitted_first_line = false;
            }
            Tag::Emphasis => {
                self.push_style_mod(|s| s.add_modifier(Modifier::ITALIC));
            }
            Tag::Strong => {
                self.push_style_mod(|s| s.add_modifier(Modifier::BOLD));
            }
            Tag::Strikethrough => {
                self.push_style_mod(|s| {
                    s.fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT)
                });
            }
            Tag::Link { dest_url, .. } => {
                self.link_dest = Some(dest_url.to_string());
                self.push_style_mod(|s| s.fg(Color::Blue).add_modifier(Modifier::UNDERLINED));
            }
            Tag::Table(alignments) => {
                self.flush_inline_buffer(false);
                self.emit_blank_if_needed();
                self.table = Some(TableState {
                    alignments,
                    header: Vec::new(),
                    rows: Vec::new(),
                    current_row: Vec::new(),
                    in_head: false,
                });
            }
            Tag::TableHead => {
                if let Some(ref mut t) = self.table {
                    t.in_head = true;
                    t.current_row.clear();
                }
            }
            Tag::TableRow => {
                if let Some(ref mut t) = self.table {
                    t.current_row.clear();
                }
            }
            Tag::TableCell => {
                self.buf.clear();
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_inline_buffer(false);
                if self.lists.is_empty() {
                    self.needs_blank_before_block = true;
                }
            }
            TagEnd::Heading(_) => {
                self.flush_inline_buffer(true);
                self.pop_style();
                self.heading_level = None;
                self.needs_blank_before_block = true;
            }
            TagEnd::BlockQuote(_) => {
                self.flush_inline_buffer(false);
                self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
                self.pop_style();
                if self.blockquote_depth == 0 {
                    self.needs_blank_before_block = true;
                }
            }
            TagEnd::CodeBlock => {
                self.flush_code_block();
                self.in_code_block = false;
                self.code_lang.clear();
                self.code_buf.clear();
                if self.lists.is_empty() {
                    self.needs_blank_before_block = true;
                }
            }
            TagEnd::List(_) => {
                self.flush_inline_buffer(false);
                self.lists.pop();
                if self.lists.is_empty() {
                    self.needs_blank_before_block = true;
                }
            }
            TagEnd::Item => {
                self.flush_inline_buffer(false);
                self.pending_list_marker = None;
                self.item_emitted_first_line = false;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.pop_style();
            }
            TagEnd::Link => {
                self.pop_style();
                if let Some(url) = self.link_dest.take() {
                    let trimmed = url.trim();
                    let label_matches = self
                        .buf
                        .last()
                        .is_some_and(|s| s.content.trim() == trimmed);
                    if !trimmed.is_empty() && !label_matches {
                        push_merged_span(
                            &mut self.buf,
                            &format!(" ({trimmed})"),
                            Style::default().fg(Color::DarkGray),
                        );
                    }
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.buf);
                if let Some(ref mut t) = self.table {
                    t.current_row.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(ref mut t) = self.table {
                    t.header = std::mem::take(&mut t.current_row);
                    t.in_head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(ref mut t) = self.table {
                    let row = std::mem::take(&mut t.current_row);
                    if !row.is_empty() {
                        t.rows.push(row);
                    }
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.render_table(t);
                    self.needs_blank_before_block = true;
                }
            }
            _ => {}
        }
    }

    fn finish(mut self) {
        self.flush_inline_buffer(false);
        if self.in_code_block && !self.code_buf.is_empty() {
            self.flush_code_block();
        }
        if let Some(t) = self.table.take() {
            self.render_table(t);
        }
    }

    fn build_prefixes(&mut self, is_heading: bool) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
        let mut first = vec![Span::raw("  ")];
        let mut cont = vec![Span::raw("  ")];

        for _ in 0..self.blockquote_depth {
            let bar = Span::styled(
                "┃ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            first.push(bar.clone());
            cont.push(bar);
        }

        if is_heading && let Some(level) = self.heading_level {
            let (glyph, color) = match level {
                HeadingLevel::H1 => ("▌ ", Color::Cyan),
                HeadingLevel::H2 => ("▸ ", Color::Blue),
                HeadingLevel::H3 => ("▪ ", Color::Cyan),
                _ => ("· ", Color::Magenta),
            };
            first.push(Span::styled(
                glyph.to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            cont.push(Span::raw(" ".repeat(glyph.width())));
            return (first, cont);
        }

        if let Some((marker, style, depth)) = self.pending_list_marker.take() {
            let indent = "  ".repeat(depth);
            let marker_w = marker.width();
            if !indent.is_empty() {
                first.push(Span::raw(indent.clone()));
                cont.push(Span::raw(indent));
            }
            first.push(Span::styled(marker, style));
            cont.push(Span::raw(" ".repeat(marker_w)));
            self.item_emitted_first_line = true;
        } else if !self.lists.is_empty() {
            let depth = self.lists.len();
            let pad = "  ".repeat(depth);
            first.push(Span::raw(pad.clone()));
            cont.push(Span::raw(pad));
        }

        (first, cont)
    }

    fn flush_inline_buffer(&mut self, is_heading: bool) {
        if self.buf.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.buf);
        let (first_prefix, cont_prefix) = self.build_prefixes(is_heading);
        wrap_styled_spans(&spans, &first_prefix, &cont_prefix, self.max_w, self.out);
    }

    fn flush_code_block(&mut self) {
        let indent = format!(
            "  {}{}",
            "  ".repeat(self.lists.len()),
            "┃ ".repeat(self.blockquote_depth)
        );
        let indent_w = indent.width();
        let inner_max = self.max_w.saturating_sub(indent_w + 3).max(16);

        let lang_label = if self.code_lang.is_empty() {
            String::new()
        } else {
            format!(" {} ", self.code_lang)
        };
        let border_len = inner_max.min(60);
        let top_dash = border_len.saturating_sub(lang_label.width() + 2);

        let mut header = vec![Span::raw(indent.clone())];
        header.push(Span::styled("┌─", Style::default().fg(Color::DarkGray)));
        if !lang_label.is_empty() {
            header.push(Span::styled(
                lang_label,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        header.push(Span::styled(
            "─".repeat(top_dash),
            Style::default().fg(Color::DarkGray),
        ));
        self.out.push(Line::from(header));

        let raw_code = self.code_buf.trim_end_matches('\n');
        let code_style = Style::default().fg(Color::Green);
        let gutter = Span::styled("│ ", Style::default().fg(Color::DarkGray));

        if raw_code.is_empty() {
            self.out.push(Line::from(vec![
                Span::raw(indent.clone()),
                gutter.clone(),
            ]));
        } else {
            for line in raw_code.lines() {
                if line.is_empty() {
                    self.out.push(Line::from(vec![
                        Span::raw(indent.clone()),
                        gutter.clone(),
                    ]));
                    continue;
                }
                let mut chunks = Vec::new();
                split_by_display_width(line, inner_max, &mut chunks);
                for chunk in chunks {
                    self.out.push(Line::from(vec![
                        Span::raw(indent.clone()),
                        gutter.clone(),
                        Span::styled(chunk, code_style),
                    ]));
                }
            }
        }

        self.out.push(Line::from(vec![
            Span::raw(indent),
            Span::styled(
                format!("└{}", "─".repeat(border_len)),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    fn render_table(&mut self, table: TableState) {
        let ncols = table
            .header
            .len()
            .max(table.rows.iter().map(Vec::len).max().unwrap_or(0));
        if ncols == 0 {
            return;
        }

        let indent = "  ";
        let avail = self.max_w.saturating_sub(indent.len()).max(20);
        let chrome = ncols * 3 + 1;
        let max_content_total = avail.saturating_sub(chrome).max(ncols * 4);

        let mut col_w = vec![3usize; ncols];
        for (c, cell) in table.header.iter().enumerate() {
            col_w[c] = col_w[c].max(spans_width(cell));
        }
        for row in &table.rows {
            for (c, cell) in row.iter().enumerate() {
                if c < ncols {
                    col_w[c] = col_w[c].max(spans_width(cell));
                }
            }
        }

        let sum: usize = col_w.iter().sum();
        if sum > max_content_total {
            while col_w.iter().sum::<usize>() > max_content_total {
                if let Some((idx, &w)) = col_w.iter().enumerate().max_by_key(|&(_, w)| w) {
                    if w <= 4 {
                        break;
                    }
                    col_w[idx] -= 1;
                } else {
                    break;
                }
            }
        }

        let border_style = Style::default().fg(Color::DarkGray);
        let draw_rule = |left: &str, mid: &str, right: &str| -> Line<'static> {
            let mut s = String::from(left);
            for (i, &w) in col_w.iter().enumerate() {
                s.push_str(&"─".repeat(w + 2));
                if i + 1 < ncols {
                    s.push_str(mid);
                }
            }
            s.push_str(right);
            Line::from(vec![
                Span::raw(indent.to_string()),
                Span::styled(s, border_style),
            ])
        };

        self.out.push(draw_rule("┌", "┬", "┐"));

        if !table.header.is_empty() {
            let header_cells: Vec<Vec<Span<'static>>> = (0..ncols)
                .map(|c| {
                    table
                        .header
                        .get(c)
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|sp| {
                            Span::styled(
                                sp.content,
                                sp.style.fg(Color::Cyan).add_modifier(Modifier::BOLD),
                            )
                        })
                        .collect()
                })
                .collect();
            render_table_row(
                indent,
                &header_cells,
                &col_w,
                &table.alignments,
                border_style,
                self.out,
            );
            self.out.push(draw_rule("├", "┼", "┤"));
        }

        for row in &table.rows {
            let cells: Vec<Vec<Span<'static>>> = (0..ncols)
                .map(|c| row.get(c).cloned().unwrap_or_default())
                .collect();
            render_table_row(
                indent,
                &cells,
                &col_w,
                &table.alignments,
                border_style,
                self.out,
            );
        }

        self.out.push(draw_rule("└", "┴", "┘"));
    }
}

fn render_table_row(
    indent: &str,
    cells: &[Vec<Span<'static>>],
    col_w: &[usize],
    alignments: &[Alignment],
    border_style: Style,
    out: &mut Vec<Line<'static>>,
) {
    let mut wrapped_cols: Vec<Vec<Vec<Span<'static>>>> = Vec::with_capacity(col_w.len());
    let mut max_h = 1usize;
    for (c, &w) in col_w.iter().enumerate() {
        let mut cell_lines = Vec::new();
        let spans = cells.get(c).map(Vec::as_slice).unwrap_or(&[]);
        wrap_styled_spans_raw(spans, w.max(1), &mut cell_lines);
        if cell_lines.is_empty() {
            cell_lines.push(Vec::new());
        }
        max_h = max_h.max(cell_lines.len());
        wrapped_cols.push(cell_lines);
    }

    for line_idx in 0..max_h {
        let mut line_spans = vec![
            Span::raw(indent.to_string()),
            Span::styled("│", border_style),
        ];
        for (c, &w) in col_w.iter().enumerate() {
            line_spans.push(Span::raw(" "));
            let seg = wrapped_cols[c]
                .get(line_idx)
                .cloned()
                .unwrap_or_default();
            let used = spans_width(&seg);
            let pad = w.saturating_sub(used);
            let align = alignments.get(c).copied().unwrap_or(Alignment::None);
            match align {
                Alignment::Right => {
                    if pad > 0 {
                        line_spans.push(Span::raw(" ".repeat(pad)));
                    }
                    line_spans.extend(seg);
                }
                Alignment::Center => {
                    let left_p = pad / 2;
                    let right_p = pad - left_p;
                    if left_p > 0 {
                        line_spans.push(Span::raw(" ".repeat(left_p)));
                    }
                    line_spans.extend(seg);
                    if right_p > 0 {
                        line_spans.push(Span::raw(" ".repeat(right_p)));
                    }
                }
                _ => {
                    line_spans.extend(seg);
                    if pad > 0 {
                        line_spans.push(Span::raw(" ".repeat(pad)));
                    }
                }
            }
            line_spans.push(Span::raw(" "));
            line_spans.push(Span::styled("│", border_style));
        }
        out.push(Line::from(line_spans));
    }
}

fn heading_style(level: HeadingLevel) -> Style {
    match level {
        HeadingLevel::H1 => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        HeadingLevel::H2 => Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD),
        HeadingLevel::H3 => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        _ => Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    }
}

fn push_merged_span(buf: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = buf.last_mut()
        && last.style == style
    {
        last.content.to_mut().push_str(text);
    } else {
        buf.push(Span::styled(text.to_string(), style));
    }
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

fn wrap_styled_spans(
    spans: &[Span<'static>],
    first_prefix: &[Span<'static>],
    cont_prefix: &[Span<'static>],
    max_w: usize,
    out: &mut Vec<Line<'static>>,
) {
    let first_w = spans_width(first_prefix);
    let cont_w = spans_width(cont_prefix);
    let body_w_first = max_w.saturating_sub(first_w).max(12);
    let body_w_cont = max_w.saturating_sub(cont_w).max(12);

    let mut cur_line: Vec<Span<'static>> = first_prefix.to_vec();
    let mut cur_w = 0usize;
    let mut limit = body_w_first;
    let mut is_first = true;

    for span in spans {
        let style = span.style;
        for ch in span.content.chars() {
            if ch == '\n' {
                out.push(Line::from(std::mem::take(&mut cur_line)));
                cur_line = cont_prefix.to_vec();
                cur_w = 0;
                limit = body_w_cont;
                is_first = false;
                continue;
            }
            let cw = ch.width().unwrap_or(1);
            if cur_w + cw > limit && cur_w > 0 {
                out.push(Line::from(std::mem::take(&mut cur_line)));
                cur_line = cont_prefix.to_vec();
                cur_w = 0;
                limit = body_w_cont;
                is_first = false;
                if ch == ' ' {
                    continue;
                }
            }
            if cur_w == 0 && !is_first && ch == ' ' {
                continue;
            }
            if let Some(last) = cur_line.last_mut()
                && last.style == style
                && cur_w > 0
            {
                last.content.to_mut().push(ch);
            } else {
                cur_line.push(Span::styled(ch.to_string(), style));
            }
            cur_w += cw;
        }
    }

    if cur_w > 0 || is_first {
        out.push(Line::from(cur_line));
    }
}

fn wrap_styled_spans_raw(
    spans: &[Span<'static>],
    max_w: usize,
    out: &mut Vec<Vec<Span<'static>>>,
) {
    let limit = max_w.max(1);
    let mut cur: Vec<Span<'static>> = Vec::new();
    let mut cur_w = 0usize;

    for span in spans {
        let style = span.style;
        for ch in span.content.chars() {
            if ch == '\n' {
                out.push(std::mem::take(&mut cur));
                cur_w = 0;
                continue;
            }
            let cw = ch.width().unwrap_or(1);
            if cur_w + cw > limit && cur_w > 0 {
                out.push(std::mem::take(&mut cur));
                cur_w = 0;
                if ch == ' ' {
                    continue;
                }
            }
            if let Some(last) = cur.last_mut()
                && last.style == style
            {
                last.content.to_mut().push(ch);
            } else {
                cur.push(Span::styled(ch.to_string(), style));
            }
            cur_w += cw;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
}

fn split_by_display_width(s: &str, max_w: usize, out: &mut Vec<String>) {
    let limit = max_w.max(8);
    let mut cur = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(1);
        if w + cw > limit && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            w = 0;
        }
        cur.push(ch);
        w += cw;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_to_plain(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn bold_wrapping_never_leaks_raw_asterisks() {
        let md = "目前无法判断代码实现质量。**检索未命中不代表代码质量有问题，这是一个会跨行折行的加粗结论。**";
        let mut out = Vec::new();
        render_markdown(md, 34, &mut out);
        let plain = lines_to_plain(&out);
        let joined = plain.replace('\n', "").replace("  ", "");
        assert!(!plain.contains("**"), "raw ** leaked: {plain}");
        assert!(joined.contains("检索未命中"), "text missing: {plain}");
        assert!(out.len() >= 2, "expected wrapped lines, got {out:?}");
        let has_bold_on_line2 = out[1]
            .spans
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::BOLD));
        assert!(has_bold_on_line2, "bold style lost across wrap: {:?}", out[1]);
    }

    #[test]
    fn renders_headings_lists_code_blocks_quotes_and_tables() {
        let md = "\
### `HttpAccess` 如何记录

1. **高｜版本校验在事务外**：检查 `expectedStateVersion`。
   - 子项：确认 `apps/api/src/main.ts`
2. 查看 [文档](https://example.com/doc)

> 注意：线上日志存在双前缀输出。

```ts
if (status >= 500) logger.error('http');
```

| 字段 | 说明 |
|---|---|
| `traceId` | 链路追踪 ID |
";
        let mut out = Vec::new();
        render_markdown(md, 60, &mut out);
        let plain = lines_to_plain(&out);
        assert!(!plain.contains("###"), "raw heading leaked: {plain}");
        assert!(!plain.contains("```"), "raw fence leaked: {plain}");
        assert!(!plain.contains("**"), "raw bold leaked: {plain}");
        assert!(plain.contains("▪ HttpAccess 如何记录"), "heading missing: {plain}");
        assert!(plain.contains("1. 高｜版本校验在事务外"), "ordered list missing: {plain}");
        assert!(plain.contains("◦ 子项"), "nested bullet missing: {plain}");
        assert!(plain.contains("┃ 注意"), "blockquote missing: {plain}");
        assert!(plain.contains("┌─ ts"), "code block header missing: {plain}");
        assert!(plain.contains("traceId") && plain.contains("链路追踪"), "table missing: {plain}");
    }

    #[test]
    fn cjk_bold_adjacent_to_punctuation_renders_cleanly() {
        let md = "检查**版本校验（事务外）**导致并发问题，以及状态为**「异常」**时重试。";
        let mut out = Vec::new();
        render_markdown(md, 80, &mut out);
        let plain = lines_to_plain(&out);
        assert!(!plain.contains("**"), "raw ** leaked in CJK punct case: {plain}");
        assert!(plain.contains("版本校验（事务外）"));
        assert!(plain.contains("「异常」"));
    }
}
