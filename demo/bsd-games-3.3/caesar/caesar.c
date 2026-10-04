// This file is free software, distributed under the BSD license.

#include "config.h"

enum { ALPHABETSZ = 'Z'-'A'+1 };

static char rotateit (char ch, unsigned rot)
{
    if (ch >= 'A' && ch <= 'Z')
	return 'A' + (unsigned)(ch - 'A' + rot) % ALPHABETSZ;
    else if (ch >= 'a' && ch <= 'z')
	return 'a' + (unsigned)(ch - 'a' + rot) % ALPHABETSZ;
    return ch;
}

static unsigned find_best_rotation (const char* s, unsigned sz)
{
    // Fill the observed frequency table
    unsigned short counts [ALPHABETSZ] = { 0 };
    unsigned nonletters = 0;
    for (unsigned i = sz; i--;) {
	char ch = s[i];
	if (ch >= 'a' && ch <= 'z')
	    ch -= 'a' - 'A';
	if (ch >= 'A' && ch <= 'Z')
	    ++counts[ch - 'A'];
	else
	    ++nonletters;
    }
    // Normalize the frequencies
    float obs [ALPHABETSZ], nletters = sz - nonletters;
    for (unsigned i = 0; i < ALPHABETSZ; ++i)
	obs[i] = counts[i] / nletters;

    // Letter frequencies from Wikipedia (cited from Robert Lewand's Cryptological Mathematics)
    static const float stdfreqs [ALPHABETSZ] = {
	0.08167, 0.01492, 0.02782, 0.04253, 0.12702,
	0.02228, 0.02015, 0.06094, 0.06966, 0.00153,
	0.00772, 0.04025, 0.02406, 0.06749, 0.07507,
	0.01929, 0.00095, 0.05987, 0.06327, 0.09056,
	0.02758, 0.00978, 0.02361, 0.00150, 0.01974,
	0.00074
    };

    // now "dot" the freqs with the observed letter freqs
    // and keep track of best fit
    float winnerdot = -1;
    unsigned winner = 0;
    for (unsigned try = 0; try < ALPHABETSZ; ++try) {
	float dot = 0;
	for (unsigned i = 0; i < ALPHABETSZ; ++i)
	    dot += obs[i] * stdfreqs [(i + try) % ALPHABETSZ];
	if (dot > winnerdot) {
	    winner = try;
	    winnerdot = dot;
	}
    }
    return winner;
}

int main (int argc, const char* const* argv)
{
    unsigned rot = UINT_MAX;

    // Check if invoked as rot13, with rotation value 13
    size_t prognamelen = strlen (argv[0]);
    if (prognamelen >= 5 && 0 == strcmp(&argv[0][prognamelen - 5], "rot13"))
	rot = 13;

    // Check if invoked with a known rotation value
    if (argc > 1)
	rot = atoi (argv[1]);

    char inbuf [BUFSIZ];
    for (unsigned nread = UINT_MAX; (nread = read (STDIN_FILENO, inbuf, BUFSIZ)) > 0;) {
	if (nread > BUFSIZ) {
	    perror ("read stdin");
	    return EXIT_FAILURE;
	}
	if (rot == UINT_MAX)
	    rot = find_best_rotation (inbuf, nread);
	for (unsigned i = 0; i < nread; ++i)
	    inbuf[i] = rotateit (inbuf[i], rot);
	write (STDOUT_FILENO, inbuf, nread);
    }
    return EXIT_SUCCESS;
}
