#!/usr/bin/env python3

import matplotlib.pyplot as plt
import upsetplot
import sys

if __name__ == '__main__':
    if len(sys.argv) != 2:
        print("usage: plot_upset.py <output file name>")

    data = dict()
    for line in sys.stdin.readlines():
        split_colon = line.strip().split(':')
        assert len(split_colon) == 2, "expected format is <filename>: [<hashes>, ]"

        filename = split_colon[0]
        hashes = set( a.strip() for a in split_colon[1].strip().split(',') if len(a) > 0)

        data[filename] = hashes

    upset_data = upsetplot.from_contents(data)
    upset = upsetplot.plot(upset_data, show_counts='{:,}')
    plt.suptitle('Intersection of private data among jif snapshots')
    plt.savefig(sys.argv[1])
