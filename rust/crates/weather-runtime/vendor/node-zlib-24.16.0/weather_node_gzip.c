/* Narrow caller-owned-buffer binding for the candidate Node compressor. */
#include "zlib.h"
#include <stdint.h>

/* Complete exactly one Node-compatible gzip level-nine member. */
int weather_node_gzip(const uint8_t *input, uint32_t input_size,
                      uint8_t *output, uint32_t output_size,
                      uint32_t *written) {
    z_stream stream = {0};
    // retain the default Node window, memLevel, strategy and zero mtime
    if (deflateInit2(&stream, 9, Z_DEFLATED, 31, 8, Z_DEFAULT_STRATEGY) != Z_OK)
        return 0;
    stream.next_in = (Bytef *)input;
    stream.avail_in = input_size;
    stream.next_out = output;
    stream.avail_out = output_size;
    int result = deflate(&stream, Z_FINISH);
    uint32_t count = (uint32_t)stream.total_out;
    // always release the initialized stream before reporting success
    if (deflateEnd(&stream) != Z_OK || result != Z_STREAM_END)
        return 0;
    *written = count;
    return 1;
}

/* Keep the same pinned compressor state across bounded source/sink chunks. */
#include <stdlib.h>

/* Allocate only the original zlib state, never the whole input or output. */
void *weather_node_gzip_stream_new(void) {
    z_stream *stream = calloc(1, sizeof(*stream));
    if (stream == NULL)
        return NULL;
    if (deflateInit2(stream, 9, Z_DEFLATED, 31, 8, Z_DEFAULT_STRATEGY) != Z_OK) {
        free(stream);
        return NULL;
    }
    return stream;
}

/* Return 1 for progress, 2 for complete stream, and 0 for an ordinary failure. */
int weather_node_gzip_stream_step(void *state, const uint8_t *input,
                                  uint32_t input_size, uint8_t *output,
                                  uint32_t output_size, int finish,
                                  uint32_t *consumed, uint32_t *written) {
    z_stream *stream = state;
    stream->next_in = (Bytef *)input;
    stream->avail_in = input_size;
    stream->next_out = output;
    stream->avail_out = output_size;
    int status = deflate(stream, finish ? Z_FINISH : Z_NO_FLUSH);
    *consumed = input_size - stream->avail_in;
    *written = output_size - stream->avail_out;
    if (status == Z_STREAM_END)
        return 2;
    return status == Z_OK ? 1 : 0;
}

/* Release the private state on success or any partial-output failure. */
void weather_node_gzip_stream_free(void *state) {
    z_stream *stream = state;
    if (stream != NULL) {
        deflateEnd(stream);
        free(stream);
    }
}
