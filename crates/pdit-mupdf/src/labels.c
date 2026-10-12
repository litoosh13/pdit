/* pdit-mupdf: Page numbers (page labels, D-070) — the PDF's /PageLabels replaced by the given ranges, inside
 * fz_try so a MuPDF error comes back as a message instead of ending the app. */
#include <mupdf/fitz.h>
#include <mupdf/pdf.h>
#include <stdio.h>

/* `data` with its page labels replaced: range k starts at page firsts[k] (from 0), style styles[k] (0, 'D', 'r',
 * 'R', 'a', 'A'), prefix prefixes[k], numbers from starts[k]. No ranges: the labels are removed (1, 2, 3).
 * On success *out holds the new file (the caller drops it with fz_drop_buffer); on failure returns -1 with `err`. */
int pdit_set_page_labels(fz_context *ctx, const unsigned char *data, size_t len, int n, const int *firsts,
			 const int *styles, const char **prefixes, const int *starts, fz_buffer **out, char *err,
			 size_t errlen)
{
	fz_stream *in = NULL;
	pdf_document *doc = NULL;
	fz_output *o = NULL;
	fz_buffer *buf = NULL;
	int code = 0;
	*out = NULL;
	fz_var(in);
	fz_var(doc);
	fz_var(o);
	fz_var(buf);
	fz_try(ctx)
	{
		in = fz_open_memory(ctx, data, len);
		doc = pdf_open_document_with_stream(ctx, in);
		pdf_obj *root = pdf_dict_get(ctx, pdf_trailer(ctx, doc), PDF_NAME(Root));
		pdf_dict_del(ctx, root, PDF_NAME(PageLabels));
		for (int k = 0; k < n; k++)
			pdf_set_page_labels(ctx, doc, firsts[k], (pdf_page_label_style)styles[k], prefixes[k], starts[k]);
		pdf_write_options w = pdf_default_write_options;
		w.do_garbage = 1;
		w.do_compress = 1;
		buf = fz_new_buffer(ctx, len + 1024);
		o = fz_new_output_with_buffer(ctx, buf);
		pdf_write_document(ctx, doc, o, &w);
		fz_close_output(ctx, o);
	}
	fz_always(ctx)
	{
		fz_drop_output(ctx, o);
		pdf_drop_document(ctx, doc);
		fz_drop_stream(ctx, in);
	}
	fz_catch(ctx)
	{
		fz_drop_buffer(ctx, buf);
		buf = NULL;
		snprintf(err, errlen, "%s", fz_caught_message(ctx));
		code = -1;
	}
	*out = buf;
	return code;
}
