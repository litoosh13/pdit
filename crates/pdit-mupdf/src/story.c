/* pdit-mupdf: MuPDF's Story (HTML/CSS layout with HarfBuzz shaping) drawn onto an existing PDF page, inside
 * fz_try so a MuPDF error comes back as a message instead of ending the app. */
#include <mupdf/fitz.h>
#include <mupdf/pdf.h>
#include <stdio.h>
#include <string.h>

/* A PDF delimiter or white space: where a name token in a content stream ends. */
static int ends_name(unsigned char c)
{
    return c == 0 || strchr(" \t\r\n\f()<>[]{}/%", c) != NULL;
}

/* `src` with every name token "/old[i]" written as "/new[i]". */
static fz_buffer *rename_names(fz_context *ctx, fz_buffer *src, char (*old)[64], char (*new)[64], int count)
{
    unsigned char *data;
    size_t len = fz_buffer_storage(ctx, src, &data);
    fz_buffer *out = fz_new_buffer(ctx, len + 64);
    size_t i = 0;
    while (i < len)
    {
        if (data[i] == '/')
        {
            size_t j = i + 1;
            while (j < len && !ends_name(data[j]))
            {
                j++;
            }
            int hit = -1;
            for (int k = 0; k < count; k++)
            {
                if (strlen(old[k]) == j - i - 1 && memcmp(old[k], data + i + 1, j - i - 1) == 0)
                {
                    hit = k;
                    break;
                }
            }
            fz_append_byte(ctx, out, '/');
            if (hit >= 0)
            {
                fz_append_string(ctx, out, new[hit]);
            }
            else
            {
                fz_append_data(ctx, out, data + i + 1, j - i - 1);
            }
            i = j;
        }
        else
        {
            fz_append_byte(ctx, out, data[i]);
            i++;
        }
    }
    return out;
}

/* What each glyph the Story shows stands for: the letters HarfBuzz shaped it from (P-046). */
typedef struct
{
    fz_font *font;
    int gid;
    int len;
    int ucs[8];
} letters;

typedef struct
{
    fz_device super;
    fz_device *target;
    letters *list;
    int len, cap;
} letters_device;

static letters *letters_of(fz_context *ctx, letters_device *d, fz_font *font, int gid)
{
    for (int i = 0; i < d->len; i++)
    {
        if (d->list[i].font == font && d->list[i].gid == gid)
        {
            return &d->list[i];
        }
    }
    if (d->len == d->cap)
    {
        d->cap = d->cap ? d->cap * 2 : 256;
        d->list = fz_realloc_array(ctx, d->list, d->cap, letters);
    }
    letters *l = &d->list[d->len++];
    memset(l, 0, sizeof *l);
    l->font = font;
    l->gid = gid;
    return l;
}

static void reverse_rtl_spans(fz_text *text)
{
    for (fz_text_span *span = text->head; span; span = span->next)
    {
        if (span->bidi_level & 1)
        {
            for (int a = 0, b = span->len - 1; a < b; a++, b--)
            {
                fz_text_item item = span->items[a];
                span->items[a] = span->items[b];
                span->items[b] = item;
            }
        }
    }
}

static void letters_fill_text(fz_context *ctx, fz_device *dev, const fz_text *text, fz_matrix ctm, fz_colorspace *cs,
                              const float *color, float alpha, fz_color_params params)
{
    letters_device *d = (letters_device *)dev;
    for (const fz_text_span *span = text->head; span; span = span->next)
    {
        letters *last = NULL;
        int fresh = 0;
        for (int i = 0; i < span->len; i++)
        {
            int gid = span->items[i].gid, ucs = span->items[i].ucs;
            if (gid >= 0)
            {
                /* A glyph seen before keeps its first reading. */
                last = letters_of(ctx, d, span->font, gid);
                fresh = last->len == 0;
            }
            if (last && fresh && ucs > 0 && last->len < 8)
            {
                last->ucs[last->len++] = ucs;
            }
        }
    }
    /* A right-to-left span goes to the PDF left to right, as PDF readers expect it: PDFium mirrors each word otherwise.
     * MuPDF has no call to copy a text, and this one is only the Story's for this drawing. */
    reverse_rtl_spans((fz_text *)text);
    fz_fill_text(ctx, d->target, text, ctm, cs, color, alpha, params);
    reverse_rtl_spans((fz_text *)text);
}

/* Adds to font `font_obj`'s ToUnicode what its glyphs really stand for. The font's own map comes from its cmap, which
 * doesn't know shaped glyphs: joined Persian letters came out as presentation forms or nothing. Definitions further down
 * a CMap replace earlier ones, so this also works on a font already used by an earlier edit.
 * ponytail: one glyph used for two different letters reads as the first one, e.g. the half-space (ZWNJ), which HarfBuzz
 * draws as the space glyph, reads as a space; ActualText per run if that matters. */
static void add_letters(fz_context *ctx, pdf_document *doc, pdf_obj *font_obj, const letters_device *d)
{
    pdf_obj *cmap = pdf_dict_get(ctx, font_obj, PDF_NAME(ToUnicode));
    const char *base = pdf_to_name(ctx, pdf_dict_get(ctx, font_obj, PDF_NAME(BaseFont)));
    if (!pdf_is_stream(ctx, cmap))
    {
        return;
    }
    fz_buffer *old = NULL, *out = NULL;
    fz_var(old);
    fz_var(out);
    fz_try(ctx)
    {
        old = pdf_load_stream(ctx, cmap);
        unsigned char *data;
        size_t len = fz_buffer_storage(ctx, old, &data);
        size_t at = len;
        for (size_t i = 0; i + 7 <= len; i++)
        {
            if (memcmp(data + i, "endcmap", 7) == 0)
            {
                at = i;
            }
        }
        out = fz_new_buffer(ctx, len + 4096);
        fz_append_data(ctx, out, data, at);
        int done = 0, added = 0;
        while (done < d->len)
        {
            int block = 0;
            fz_buffer *chars = fz_new_buffer(ctx, 2048);
            fz_try(ctx)
            {
                for (; done < d->len && block < 100; done++)
                {
                    const letters *l = &d->list[done];
                    if (l->len == 0 || strcmp(fz_font_name(ctx, l->font), base) != 0)
                    {
                        continue;
                    }
                    fz_append_printf(ctx, chars, "<%04x> <", l->gid);
                    for (int k = 0; k < l->len; k++)
                    {
                        int c = l->ucs[k];
                        if (c >= 0x10000)
                        {
                            fz_append_printf(ctx, chars, "%04x%04x", 0xD800 + ((c - 0x10000) >> 10),
                                             0xDC00 + ((c - 0x10000) & 0x3FF));
                        }
                        else
                        {
                            fz_append_printf(ctx, chars, "%04x", c);
                        }
                    }
                    fz_append_string(ctx, chars, ">\n");
                    block++;
                }
                if (block)
                {
                    fz_append_printf(ctx, out, "%d beginbfchar\n", block);
                    fz_append_buffer(ctx, out, chars);
                    fz_append_string(ctx, out, "endbfchar\n");
                    added += block;
                }
            }
            fz_always(ctx)
            {
                fz_drop_buffer(ctx, chars);
            }
            fz_catch(ctx)
            {
                fz_rethrow(ctx);
            }
        }
        if (added)
        {
            fz_append_data(ctx, out, data + at, len - at);
            pdf_update_stream(ctx, doc, cmap, out, 0);
        }
    }
    fz_always(ctx)
    {
        fz_drop_buffer(ctx, old);
        fz_drop_buffer(ctx, out);
    }
    fz_catch(ctx)
    {
        fz_rethrow(ctx);
    }
}

/* Lays out `html` (styled by `css`; fonts named in the CSS come from the `nfonts` buffers, by name) in `where`
 * (page space: points, origin top-left) and appends the drawing to the page's content as a new stream. It is drawn
 * with resources of its own first, which are then added to the page's under new names ("Pdit" + name + number),
 * so the fonts it adds can't take the names of the page's own fonts, and the text stays on the page itself (not
 * inside a form XObject), where every reader and editor finds it.
 * `filled` gets the area used; `more` is set to 1 when the text did not fit in `where`; `xname` gets the new font
 * resource names, separated by spaces (`xn` bytes). `mark`, when not NULL, is a marked-content opening
 * (`/Tag <<…>> BDC`) put around the new text, closed with EMC.
 * Returns 0, or -1 with the error message in `err` (`n` bytes). */
int pdit_story_onto_page(fz_context *ctx, pdf_document *doc, pdf_page *page, fz_rect where, const char *html,
                         const char *css, const char *mark, const char **names, const unsigned char **datas, const size_t *lens,
                         int nfonts, fz_rect *filled, int *more, char *xname, size_t xn, char *err, size_t n)
{
    fz_archive *arch = NULL;
    fz_buffer *html_buf = NULL;
    fz_buffer *font_buf = NULL;
    fz_story *story = NULL;
    fz_buffer *contents = NULL;
    fz_device *dev = NULL;
    pdf_obj *stream = NULL;
    pdf_obj *array = NULL;
    pdf_obj *xres = NULL;
    fz_buffer *show = NULL;
    letters_device *seen = NULL;
    int code = 0;

    fz_var(arch);
    fz_var(html_buf);
    fz_var(font_buf);
    fz_var(story);
    fz_var(contents);
    fz_var(dev);
    fz_var(stream);
    fz_var(array);
    fz_var(xres);
    fz_var(show);
    fz_var(seen);

    fz_try(ctx)
    {
        arch = fz_new_tree_archive(ctx, NULL);
        for (int i = 0; i < nfonts; i++)
        {
            font_buf = fz_new_buffer_from_copied_data(ctx, datas[i], lens[i]);
            fz_tree_archive_add_buffer(ctx, arch, names[i], font_buf);
            fz_drop_buffer(ctx, font_buf);
            font_buf = NULL;
        }
        html_buf = fz_new_buffer_from_copied_data(ctx, (const unsigned char *)html, strlen(html));
        story = fz_new_story(ctx, html_buf, css, 12, arch);
        *more = fz_place_story(ctx, story, where, filled);

        /* The Story draws in page space; the PDF device writes user space (bottom-left origin, unrotated). */
        fz_rect mediabox;
        fz_matrix page_ctm;
        pdf_page_transform(ctx, page, &mediabox, &page_ctm);
        contents = fz_new_buffer(ctx, 1024);
        xres = pdf_new_dict(ctx, doc, 2);
        dev = pdf_new_pdf_device(ctx, doc, fz_invert_matrix(page_ctm), xres, contents);
        /* Story draws only text here, so that's all this device passes on to the PDF device. */
        seen = fz_new_derived_device(ctx, letters_device);
        seen->super.fill_text = letters_fill_text;
        seen->target = dev;
        fz_draw_story(ctx, story, &seen->super, fz_identity);
        fz_close_device(ctx, &seen->super);
        fz_close_device(ctx, dev);

        pdf_obj *resources = pdf_page_resources(ctx, page);
        if (!resources)
        {
            resources = pdf_dict_put_dict(ctx, page->obj, PDF_NAME(Resources), 2);
        }
        /* Every resource the drawing uses, under a name the page doesn't have yet. */
        char old_names[64][64];
        char new_names[64][64];
        int count = 0;
        xname[0] = 0;
        for (int c = 0; c < pdf_dict_len(ctx, xres); c++)
        {
            pdf_obj *kind = pdf_dict_get_key(ctx, xres, c);
            pdf_obj *src = pdf_dict_get_val(ctx, xres, c);
            pdf_obj *dst = pdf_dict_get(ctx, resources, kind);
            if (!dst)
            {
                dst = pdf_dict_put_dict(ctx, resources, kind, 4);
            }
            for (int i = 0; i < pdf_dict_len(ctx, src) && count < 64; i++)
            {
                const char *name = pdf_to_name(ctx, pdf_dict_get_key(ctx, src, i));
                for (int k = 0;; k++)
                {
                    snprintf(new_names[count], 64, "Pdit%s_%d", name, k);
                    if (!pdf_dict_gets(ctx, dst, new_names[count]))
                    {
                        break;
                    }
                }
                snprintf(old_names[count], 64, "%s", name);
                pdf_dict_puts(ctx, dst, new_names[count], pdf_dict_get_val(ctx, src, i));
                if (pdf_name_eq(ctx, kind, PDF_NAME(Font)))
                {
                    add_letters(ctx, doc, pdf_dict_get_val(ctx, src, i), seen);
                }
                if (pdf_name_eq(ctx, kind, PDF_NAME(Font)) && strlen(xname) + strlen(new_names[count]) + 2 < xn)
                {
                    strcat(xname, new_names[count]);
                    strcat(xname, " ");
                }
                count++;
            }
        }
        fz_buffer *renamed = rename_names(ctx, contents, old_names, new_names, count);
        fz_drop_buffer(ctx, contents);
        contents = renamed;
        show = fz_new_buffer(ctx, fz_buffer_storage(ctx, contents, NULL) + 8);
        fz_append_string(ctx, show, "q\n");
        if (mark)
        {
            fz_append_printf(ctx, show, "%s\n", mark);
        }
        fz_append_buffer(ctx, show, contents);
        fz_append_string(ctx, show, mark ? "\nEMC\nQ\n" : "\nQ\n");
        stream = pdf_add_stream(ctx, doc, show, NULL, 0);
        pdf_obj *old = pdf_dict_get(ctx, page->obj, PDF_NAME(Contents));
        if (pdf_is_array(ctx, old))
        {
            pdf_array_push(ctx, old, stream);
        }
        else
        {
            array = pdf_new_array(ctx, doc, 2);
            if (old)
            {
                pdf_array_push(ctx, array, old);
            }
            pdf_array_push(ctx, array, stream);
            pdf_dict_put(ctx, page->obj, PDF_NAME(Contents), array);
        }
    }
    fz_always(ctx)
    {
        pdf_drop_obj(ctx, array);
        pdf_drop_obj(ctx, xres);
        fz_drop_buffer(ctx, show);
        pdf_drop_obj(ctx, stream);
        fz_drop_device(ctx, dev);
        if (seen)
        {
            fz_free(ctx, seen->list);
        }
        fz_drop_device(ctx, seen ? &seen->super : NULL);
        fz_drop_buffer(ctx, contents);
        fz_drop_story(ctx, story);
        fz_drop_buffer(ctx, html_buf);
        fz_drop_buffer(ctx, font_buf);
        fz_drop_archive(ctx, arch);
    }
    fz_catch(ctx)
    {
        snprintf(err, n, "%s", fz_caught_message(ctx));
        code = -1;
    }
    return code;
}
