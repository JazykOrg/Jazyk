/**
 * Sorting Algorithm CLI Utility
 * This script reads lines from STDIN, strips whitespace, skips empty lines, 
 * sorts the valid lines (ascending or descending based on -r flag), and prints the result.
 */

const args = process.argv.slice(2);
let isReverse = false;

// Check for -r argument (req:main-js-2)
if (args.includes('-r')) {
    isReverse = true; // req:main-js-3
}

const lines = [];

// Read all data from STDIN
process.stdin.on('data', (chunk) => {
    const chunkString = chunk.toString();
    const linesInChunk = chunkString.split('\n');

    for (const line of linesInChunk) {
        // Strip whitespace before and after the current line read from STDIN (req:main-js-5)
        const strippedLine = line.trim(); 

        // If stripped line is empty string, continue to next line (req:main-js-6)
        if (strippedLine === '') {
            continue;
        }

        // Add stripped line to lines (req:main-js-7)
        lines.push(strippedLine);
    }
});

process.stdin.on('end', () => {
    let sortedLines = [...lines]; // Create a copy for sorting

    // req:main-js-8: Sort lines ascending or descending if reverse order is set.
    if (isReverse) {
        // Descending sort: b compared to a
        sortedLines.sort((a, b) => b.localeCompare(a));
    } else {
        // Ascending sort: a compared to b
        sortedLines.sort((a, b) => a.localeCompare(b));
    }

    // req:main-js-9: Print all sorted lines delimited by newline.
    console.log(sortedLines.join('\n'));
});
