class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.9.0"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.9.0/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "884da0c16c0f26549e5534de9d28407e1b8f51214d86ec81e99f0ce51c844c64"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.9.0/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "47c4ce1293b0ebdfba3afef278adc734c3bae5002ecbe07813076d6b61e197ed"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.9.0/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "a1b3f36e8465f7b36edc407934bc9febaeb15bcd261b3d924b674e83a6533036"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.9.0/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "386a4ea0e708aa47fe4777e6298ccadb5891ef8762eb673555af7798bb4d1f84"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
