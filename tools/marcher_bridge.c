#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

/* The comparison target is compiled from a separately pinned checkout. */
#include "board_search.c"

int main(void) {
    uint64_t white, black, white_kings, black_kings;
    int player, forced;
    unsigned micros;
    while (scanf("%" SCNx64 " %" SCNx64 " %" SCNx64 " %" SCNx64 " %d %d %u",
                 &white, &black, &white_kings, &black_kings,
                 &player, &forced, &micros) == 7) {
        if (micros == 0 || micros > 50000 || player < 1 || player > 2 ||
            forced < -1 || forced >= 64) {
            return 2;
        }
        struct search_info *result = start_board_search(
            (long long)white, (long long)black, (long long)white_kings,
            (long long)black_kings, player, (float)micros / 1000000.0f, 40, forced);
        unsigned packed = (unsigned)result->best_move;
        printf("%u %u %llu\n", (packed >> 8) & 255u, packed & 255u,
               (unsigned long long)result->evaler->nodes);
        fflush(stdout);
        end_board_search(result->evaler);
        free(result);
    }
    return ferror(stdin) ? 3 : 0;
}
