/* pdit-mupdf: Compress PDF (M-009, D-069) — MuPDF's own cleaner (as `mutool clean`), inside fz_try so a MuPDF
 * error comes back as a message instead of ending the app. Tested in .claude/research/compress-spike. */
#include <mupdf/fitz.h>
#include <mupdf/pdf.h>
#include <stdio.h>
#include <string.h>

/* Pictures above `above` dpi are scaled down to `to` dpi and stored as JPEG at `quality`, only when that is smaller. */
static void shrink_images(pdf_image_rewriter_options *i, int above, int to, char *quality)
{
	i->color_lossy_image_subsample_threshold = i->color_lossless_image_subsample_threshold = above;
	i->gray_lossy_image_subsample_threshold = i->gray_lossless_image_subsample_threshold = above;
	i->color_lossy_image_subsample_to = i->color_lossless_image_subsample_to = to;
	i->gray_lossy_image_subsample_to = i->gray_lossless_image_subsample_to = to;
	i->color_lossy_image_subsample_method = i->color_lossless_image_subsample_method = FZ_SUBSAMPLE_BICUBIC;
	i->gray_lossy_image_subsample_method = i->gray_lossless_image_subsample_method = FZ_SUBSAMPLE_BICUBIC;
	i->color_lossy_image_recompress_method = i->color_lossless_image_recompress_method = FZ_RECOMPRESS_JPEG;
	i->gray_lossy_image_recompress_method = i->gray_lossless_image_recompress_method = FZ_RECOMPRESS_JPEG;
	i->color_lossy_image_recompress_quality = i->color_lossless_image_recompress_quality = quality;
	i->gray_lossy_image_recompress_quality = i->gray_lossless_image_recompress_quality = quality;
	i->recompress_when = FZ_RECOMPRESS_WHEN_SMALLER;
}

/* `data` compressed at `level` (0 best quality: lossless; 1 medium: 150 dpi, JPEG 75; 2 smallest: 100 dpi, JPEG 60).
 * On success *out holds the new file (the caller drops it with fz_drop_buffer); on failure returns -1 with `err`. */
int pdit_compress(fz_context *ctx, const unsigned char *data, size_t len, int level, fz_buffer **out, char *err,
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
		if (level > 0)
		{
			pdf_image_rewriter_options images;
			memset(&images, 0, sizeof images);
			if (level == 1)
				shrink_images(&images, 225, 150, "75");
			else
				shrink_images(&images, 150, 100, "60");
			pdf_rewrite_images(ctx, doc, &images);
		}
		pdf_subset_fonts(ctx, doc, 0, NULL);
		pdf_write_options w = pdf_default_write_options;
		w.do_garbage = 3; /* collect, renumber, de-duplicate */
		w.do_compress = 1;
		w.do_compress_images = 1;
		w.do_compress_fonts = 1;
		w.do_use_objstms = 1;
		buf = fz_new_buffer(ctx, len / 4 + 1024);
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
