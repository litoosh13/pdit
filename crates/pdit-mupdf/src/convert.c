/* pdit-mupdf: PDF → Word (.docx) / OpenDocument (.odt) (D-071) with MuPDF's writer (Artifex "extract"), inside
 * fz_try so a MuPDF error comes back as a message instead of ending the app. Two writer bugs are worked around
 * (RESEARCH.md "Conversion and PDF/A"; still in extract upstream, 2026-10-09):
 *  - pictures: extract keeps only the last page's pictures (extract_document_images replaces its list on every
 *    page) — so all pages are drawn as one long page;
 *  - line breaks: "<w:br/>" is written inside <w:t> (docx), and as a Word element in an .odt;
 *  - picture sizes: a .docx picture has no size (extract's <wp:extent> line is commented out), an .odt one is its
 *    pixels at 72 dpi (a 300 dpi scan becomes 11 in wide);
 *  - an .odt's "mimetype" isn't first (ODF requires it first, uncompressed).
 * So the finished package is rewritten with its document XML fixed: breaks moved out of the text (odt:
 * <text:line-break/>), every picture sized and fitted to the text width, mimetype first. */
#include <mupdf/fitz.h>
#include <mupdf/pdf.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Gap between pages on the long page (points): a paragraph never runs across two pages. */
#define PAGE_GAP 72.0f

static const char *BR = "<w:br/>";
/* The text width a picture is fitted to: 6.5 in (A4 or Letter with 1 in margins). */
#define TEXT_WIDTH_IN 6.5
/* Word's unit: 914400 per inch. */
#define EMU_PER_IN 914400.0

/* The size (EMU) of .docx picture `id` (extract names it word/media/image<id>.<ext>): its pixels at 72 dpi,
 * fitted to the text width. 0 x 0 when it isn't there. */
static void picture_emu(fz_context *ctx, fz_archive *arch, int id, double *cx, double *cy)
{
	char prefix[64];
	snprintf(prefix, sizeof prefix, "word/media/image%d.", id);
	*cx = *cy = 0;
	int count = fz_count_archive_entries(ctx, arch);
	for (int k = 0; k < count; k++)
	{
		const char *name = fz_list_archive_entry(ctx, arch, k);
		if (strncmp(name, prefix, strlen(prefix)) != 0)
			continue;
		fz_buffer *buf = fz_read_archive_entry(ctx, arch, name);
		fz_image *img = NULL;
		fz_try(ctx)
		{
			img = fz_new_image_from_buffer(ctx, buf);
			*cx = img->w * EMU_PER_IN / 72.0;
			*cy = img->h * EMU_PER_IN / 72.0;
		}
		fz_always(ctx)
		{
			fz_drop_image(ctx, img);
			fz_drop_buffer(ctx, buf);
		}
		fz_catch(ctx)
		{
			fz_rethrow(ctx);
		}
		break;
	}
	if (*cx > TEXT_WIDTH_IN * EMU_PER_IN)
	{
		*cy *= TEXT_WIDTH_IN * EMU_PER_IN / *cx;
		*cx = TEXT_WIDTH_IN * EMU_PER_IN;
	}
}

/* An .odt <draw:frame ...> tag `tag`[0..len) appended with its svg:width / svg:height (inches) fitted to the
 * text width. */
static void append_fitted_frame(fz_context *ctx, fz_buffer *out, const char *tag, size_t len)
{
	char copy[1024];
	if (len >= sizeof copy)
	{
		fz_append_data(ctx, out, tag, len);
		return;
	}
	memcpy(copy, tag, len);
	copy[len] = 0;
	char *w = strstr(copy, "svg:width=\""), *h = strstr(copy, "svg:height=\"");
	double win = w ? atof(w + 11) : 0, hin = h ? atof(h + 12) : 0;
	if (!w || !h || win <= TEXT_WIDTH_IN)
	{
		fz_append_data(ctx, out, tag, len);
		return;
	}
	hin *= TEXT_WIDTH_IN / win;
	win = TEXT_WIDTH_IN;
	/* Rewrite both values (each runs to its closing quote). */
	for (const char *p = copy; *p; )
	{
		if (p == w || p == h)
		{
			int is_w = p == w;
			fz_append_printf(ctx, out, is_w ? "svg:width=\"%gin\"" : "svg:height=\"%gin\"", is_w ? win : hin);
			p += is_w ? 11 : 12;
			while (*p && *p != '"')
				p++;
			if (*p)
				p++;
		}
		else
			fz_append_byte(ctx, out, *p++);
	}
}

/* The last occurrence of `needle` in `hay`[0..end), or NULL. */
static const char *last_before(const char *hay, size_t end, const char *needle)
{
	const char *found = NULL;
	size_t n = strlen(needle);
	for (const char *p = hay; p + n <= hay + end; p++)
		if (memcmp(p, needle, n) == 0)
			found = p;
	return found;
}

/* `src` (the document XML) fixed: every <w:br/> moved out of the text (a .docx closes the text first, or wraps
 * a break outside any run in one; an .odt gets ODF's own line break), and every picture sized and fitted. */
static fz_buffer *fix_document(fz_context *ctx, fz_buffer *src, int odt, fz_archive *arch)
{
	unsigned char *data;
	/* Terminated, so the string searches below stop at its end. */
	fz_terminate_buffer(ctx, src);
	size_t len = fz_buffer_storage(ctx, src, &data);
	const char *xml = (const char *)data;
	fz_buffer *out = fz_new_buffer(ctx, len + 1024);
	size_t i = 0, n = strlen(BR);
	/* The size of the .docx picture being written, for its <a:ext> after <a:off>. */
	double cx = 0, cy = 0;
	int pending = 0;
	static const char *OFF = "<a:off x=\"0\" y=\"0\"/>";
	while (i < len)
	{
		const char *at = xml + i;
		if (i + n <= len && memcmp(at, BR, n) == 0)
		{
			if (odt)
				fz_append_string(ctx, out, "<text:line-break/>");
			else
			{
				/* Inside a text element when its last opening comes after its last closing. */
				const char *open = last_before(xml, i, "<w:t>");
				const char *open2 = last_before(xml, i, "<w:t ");
				const char *close = last_before(xml, i, "</w:t>");
				if (open2 > open)
					open = open2;
				if (open && (!close || open > close))
					fz_append_string(ctx, out, "</w:t><w:br/><w:t xml:space=\"preserve\">");
				else
					fz_append_string(ctx, out, "<w:r><w:br/></w:r>");
			}
			i += n;
		}
		else if (!odt && len - i > 10 && memcmp(at, "<wp:inline", 10) == 0)
		{
			/* The picture's size from its r:embed="rId<n>", right after the opening tag. */
			const char *end = memchr(at, '>', len - i);
			const char *embed = strstr(at, "r:embed=\"rId");
			if (!end || !embed)
			{
				fz_append_byte(ctx, out, xml[i++]);
				continue;
			}
			picture_emu(ctx, arch, atoi(embed + 12), &cx, &cy);
			fz_append_data(ctx, out, at, end - at + 1);
			fz_append_printf(ctx, out, "<wp:extent cx=\"%d\" cy=\"%d\"/>", (int)cx, (int)cy);
			pending = 1;
			i += end - at + 1;
		}
		else if (pending && len - i >= strlen(OFF) && memcmp(at, OFF, strlen(OFF)) == 0)
		{
			fz_append_string(ctx, out, OFF);
			fz_append_printf(ctx, out, "<a:ext cx=\"%d\" cy=\"%d\"/>", (int)cx, (int)cy);
			pending = 0;
			i += strlen(OFF);
		}
		else if (odt && len - i > 11 && memcmp(at, "<draw:frame", 11) == 0)
		{
			const char *end = memchr(at, '>', len - i);
			if (!end)
			{
				fz_append_byte(ctx, out, xml[i++]);
				continue;
			}
			append_fitted_frame(ctx, out, at, end - at + 1);
			i += end - at + 1;
		}
		else
			fz_append_byte(ctx, out, xml[i++]);
	}
	return out;
}

/* The package `in` rewritten entry by entry, its document XML fixed. An .odt's "mimetype" goes first and
 * uncompressed, as ODF requires (extract writes it fourth, compressed). */
static fz_buffer *repack(fz_context *ctx, fz_buffer *in, int odt)
{
	fz_stream *stm = NULL;
	fz_archive *arch = NULL;
	fz_buffer *out = NULL, *entry = NULL, *fixed = NULL;
	fz_output *o = NULL;
	fz_zip_writer *zip = NULL;
	const char *xml_name = odt ? "content.xml" : "word/document.xml";
	fz_var(stm);
	fz_var(arch);
	fz_var(out);
	fz_var(entry);
	fz_var(fixed);
	fz_var(o);
	fz_var(zip);
	fz_try(ctx)
	{
		stm = fz_open_buffer(ctx, in);
		arch = fz_open_zip_archive_with_stream(ctx, stm);
		out = fz_new_buffer(ctx, fz_buffer_storage(ctx, in, NULL) + 1024);
		o = fz_new_output_with_buffer(ctx, out);
		{
			/* The zip writer owns the output from the call on, even if it throws. */
			fz_output *zo = o;
			o = NULL;
			zip = fz_new_zip_writer_with_output(ctx, zo);
		}
		if (odt && fz_has_archive_entry(ctx, arch, "mimetype"))
		{
			entry = fz_read_archive_entry(ctx, arch, "mimetype");
			fz_write_zip_entry(ctx, zip, "mimetype", entry, 0);
			fz_drop_buffer(ctx, entry);
			entry = NULL;
		}
		int count = fz_count_archive_entries(ctx, arch);
		for (int k = 0; k < count; k++)
		{
			const char *name = fz_list_archive_entry(ctx, arch, k);
			if (odt && strcmp(name, "mimetype") == 0)
				continue;
			entry = fz_read_archive_entry(ctx, arch, name);
			if (strcmp(name, xml_name) == 0)
			{
				fixed = fix_document(ctx, entry, odt, arch);
				fz_write_zip_entry(ctx, zip, name, fixed, 1);
				fz_drop_buffer(ctx, fixed);
				fixed = NULL;
			}
			else
				fz_write_zip_entry(ctx, zip, name, entry, 1);
			fz_drop_buffer(ctx, entry);
			entry = NULL;
		}
		fz_close_zip_writer(ctx, zip);
	}
	fz_always(ctx)
	{
		fz_drop_zip_writer(ctx, zip);
		fz_drop_output(ctx, o);
		fz_drop_buffer(ctx, entry);
		fz_drop_buffer(ctx, fixed);
		fz_drop_archive(ctx, arch);
		fz_drop_stream(ctx, stm);
	}
	fz_catch(ctx)
	{
		fz_drop_buffer(ctx, out);
		fz_rethrow(ctx);
	}
	return out;
}

/* Pages `first`..=`last` (from 0) of `data` as .docx (odt = 0) or .odt (odt = 1). On success *out holds the file
 * (the caller drops it with fz_drop_buffer); on failure returns -1 with `err`. */
int pdit_convert_office(fz_context *ctx, const unsigned char *data, size_t len, int first, int last, int odt,
			fz_buffer **out, char *err, size_t errlen)
{
	fz_stream *in = NULL;
	fz_document *doc = NULL;
	fz_buffer *raw = NULL;
	fz_output *o = NULL;
	fz_document_writer *writer = NULL;
	fz_page *page = NULL;
	int code = 0;
	*out = NULL;
	fz_var(in);
	fz_var(doc);
	fz_var(raw);
	fz_var(o);
	fz_var(writer);
	fz_var(page);
	fz_try(ctx)
	{
		in = fz_open_memory(ctx, data, len);
		doc = fz_open_document_with_stream(ctx, "application/pdf", in);
		if (first < 0 || last < first || last >= fz_count_pages(ctx, doc))
			fz_throw(ctx, FZ_ERROR_ARGUMENT, "no such pages");
		/* One long page: the pages one under another. */
		float width = 0, height = 0;
		for (int p = first; p <= last; p++)
		{
			fz_rect b = fz_bound_page(ctx, page = fz_load_page(ctx, doc, p));
			fz_drop_page(ctx, page);
			page = NULL;
			width = fz_max(width, b.x1 - b.x0);
			height += (b.y1 - b.y0) + (p < last ? PAGE_GAP : 0);
		}
		raw = fz_new_buffer(ctx, len + 4096);
		o = fz_new_output_with_buffer(ctx, raw);
		{
			/* The writer owns the output from the call on, even if it throws. */
			fz_output *wo = o;
			o = NULL;
			writer = odt ? fz_new_odt_writer_with_output(ctx, wo, "") : fz_new_docx_writer_with_output(ctx, wo, "");
		}
		fz_device *dev = fz_begin_page(ctx, writer, fz_make_rect(0, 0, width, height));
		float y = 0;
		for (int p = first; p <= last; p++)
		{
			page = fz_load_page(ctx, doc, p);
			fz_rect b = fz_bound_page(ctx, page);
			fz_run_page(ctx, page, dev, fz_translate(-b.x0, y - b.y0), NULL);
			y += (b.y1 - b.y0) + PAGE_GAP;
			fz_drop_page(ctx, page);
			page = NULL;
		}
		fz_end_page(ctx, writer);
		fz_close_document_writer(ctx, writer);
		*out = repack(ctx, raw, odt);
	}
	fz_always(ctx)
	{
		fz_drop_page(ctx, page);
		fz_drop_document_writer(ctx, writer);
		fz_drop_output(ctx, o);
		fz_drop_buffer(ctx, raw);
		fz_drop_document(ctx, doc);
		fz_drop_stream(ctx, in);
	}
	fz_catch(ctx)
	{
		snprintf(err, errlen, "%s", fz_caught_message(ctx));
		code = -1;
	}
	return code;
}
