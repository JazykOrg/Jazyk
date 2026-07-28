const { exec } = require('child_process');
const util = require('util');

const execPromise = util.promisify(exec);

async function runTest(command, expectedOutput) {
    try {
        const { stdout } = await execPromise(command);
        if (stdout.trim() === expectedOutput.trim()) {
            console.log(`PASS: ${command}`);
        } else {
            console.error(`FAIL: ${command}. Expected:\n${expectedOutput}\nGot:\n${stdout}`);
            process.exit(1);
        }
    } catch (error) {
        console.error(`Error running test for ${command}:`, error);
        process.exit(1);
    }
}

// req:main-js-5 [req_main_js_5_310b7c2e]: The Sorting Algorithm CLI Utility shall strip out whitespace before and after the current line read from STDIN.
async function testStripWhitespace() {
    const command = 'echo "  apple  \nbanana\ncherry" | node index.js';
    // Expected output should be sorted alphabetically, with stripped lines
    const expectedOutput = 'apple\nbanana\ncherry'; 
    await runTest(command, expectedOutput);
}

// req:main-js-6 [req_main_js_6_3cfdde6e]: If a stripped line is an empty string, then the Sorting Algorithm CLI Utility shall skip it.
async function testSkipEmptyLines() {
    const command = 'echo "valid\n  \t\nanother" | node index.js';
    // Expected output: valid and another, sorted ascendingly
    const expectedOutput = 'another\nvalid'; 
    await runTest(command, expectedOutput);
}

// req:main-test-js-2 [req_main_test_js_2_d8b863ea]: When given the input lines 321, 654, 453, the Sorting Algorithm CLI Utility shall output 321, 453, 654.
async function testAscendingSort() {
    const command = 'echo "321\n654\n453" | node index.js';
    // Expected output: ascending order
    const expectedOutput = '321\n453\n654'; 
    await runTest(command, expectedOutput);
}

// req:main-test-js-1 [req_main_test_js_1_3e874d16]: When given the input lines 321, 654, 453, the Sorting Algorithm CLI Utility shall output 654, 453, 321.
async function testDescendingSort() {
    const command = 'echo "321\n654\n453" | node index.js -r';
    // Expected output: descending order
    const expectedOutput = '654\n453\n321'; 
    await runTest(command, expectedOutput);
}

async function main() {
    console.log("Running sorting algorithm tests...");
    await testStripWhitespace();
    await testSkipEmptyLines();
    await testAscendingSort();
    await testDescendingSort();
    console.log("\All tests passed successfully.");
}

main().catch(err => console.error('An error occurred during testing:', err));