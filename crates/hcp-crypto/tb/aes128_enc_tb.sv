// -----------------------------------------------------------------------------
// aes128_enc_tb.sv  --  self-checking testbench (FIPS-197 known-answer tests)
//
// Used both standalone (`just aes-sim`) and by the Rust integration test
// `tests/verilog_kat.rs`, which generates the RTL and runs this against it.
// SPDX-License-Identifier: Apache-2.0
// -----------------------------------------------------------------------------
`timescale 1ns/1ps
`default_nettype none

module aes128_enc_tb;

    reg          clk = 0;
    reg          rst_n = 0;
    reg          start = 0;
    reg  [127:0] key = 0;
    reg  [127:0] plaintext = 0;
    wire [127:0] ciphertext;
    wire         done;
    wire         busy;

    integer errors = 0;
    integer cycles = 0;
    integer t0, t1;

    aes128_enc dut (
        .clk(clk), .rst_n(rst_n), .start(start),
        .key(key), .plaintext(plaintext),
        .ciphertext(ciphertext), .done(done), .busy(busy)
    );

    always #5 clk = ~clk;   // 100 MHz

    // Count cycles for the latency check.
    always @(posedge clk) cycles = cycles + 1;

    task run_vector(input [127:0] k, input [127:0] pt, input [127:0] expected,
                    input [8*24:1] name);
        begin
            @(negedge clk);
            key = k; plaintext = pt; start = 1;
            t0 = cycles;
            @(negedge clk);
            start = 0;
            wait (done == 1'b1);
            t1 = cycles;
            @(negedge clk);
            if (ciphertext === expected) begin
                $display("PASS  %0s : %032h  (%0d cycles)", name, ciphertext, t1 - t0);
            end else begin
                $display("FAIL  %0s", name);
                $display("      got      %032h", ciphertext);
                $display("      expected %032h", expected);
                errors = errors + 1;
            end
            if ((t1 - t0) != 11) begin
                $display("FAIL  %0s : expected 11 cycles, got %0d", name, t1 - t0);
                errors = errors + 1;
            end
        end
    endtask

    initial begin
        repeat (3) @(negedge clk);
        rst_n = 1;
        @(negedge clk);

        // FIPS-197 Appendix C.1 -- the canonical AES-128 example
        run_vector(128'h000102030405060708090a0b0c0d0e0f,
                   128'h00112233445566778899aabbccddeeff,
                   128'h69c4e0d86a7b0430d8cdb78070b4c55a,
                   "C.1 canonical");

        // All-zero key and plaintext
        run_vector(128'h00000000000000000000000000000000,
                   128'h00000000000000000000000000000000,
                   128'h66e94bd4ef8a2c3b884cfa59ca342b2e,
                   "all-zero KP");

        // FIPS-197 Appendix B -- the step-by-step worked example
        run_vector(128'h2b7e151628aed2a6abf7158809cf4f3c,
                   128'h3243f6a8885a308d313198a2e0370734,
                   128'h3925841d02dc09fbdc118597196a0b32,
                   "App-B example");

        // Immediate reuse after done: proves the handshake resets cleanly
        run_vector(128'h000102030405060708090a0b0c0d0e0f,
                   128'h00112233445566778899aabbccddeeff,
                   128'h69c4e0d86a7b0430d8cdb78070b4c55a,
                   "reuse C.1");

        // Key change between blocks (key must not be latched stale)
        run_vector(128'h2b7e151628aed2a6abf7158809cf4f3c,
                   128'h3243f6a8885a308d313198a2e0370734,
                   128'h3925841d02dc09fbdc118597196a0b32,
                   "key switch");

        // Mid-flight reset must not corrupt the next block
        @(negedge clk);
        key = 128'h000102030405060708090a0b0c0d0e0f;
        plaintext = 128'h00112233445566778899aabbccddeeff;
        start = 1;
        @(negedge clk);
        start = 0;
        repeat (4) @(negedge clk);
        rst_n = 0;
        @(negedge clk);
        rst_n = 1;
        @(negedge clk);
        if (busy !== 1'b0) begin
            $display("FAIL  reset did not clear busy");
            errors = errors + 1;
        end
        run_vector(128'h000102030405060708090a0b0c0d0e0f,
                   128'h00112233445566778899aabbccddeeff,
                   128'h69c4e0d86a7b0430d8cdb78070b4c55a,
                   "after reset");

        if (errors == 0)
            $display("\nALL TESTS PASSED");
        else
            $display("\n%0d TEST(S) FAILED", errors);

        $finish;
    end

    initial begin
        #200000;
        $display("TIMEOUT");
        $display("\n1 TEST(S) FAILED");
        $finish;
    end

endmodule

`default_nettype wire
