class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.1"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.1/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "ee19c9ff64727b6dcd5b7bc055c0bd52fc41e94a81f885df57fbd86776212d15"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.1/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "4d69799d4140ae79cbe5a8d62c266c21e5cfa8577ba1cd96f9866d1a8fd22b7d"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.1/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "af0df796b22f40746b9e811e0dcffde1c548f2a06f384f4060c501a3c6a0f390"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.1/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "bc3d5fcc427326a60aaadd5dada42f2908ea1fc04a9b519ee49f7ca07651f61d"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
