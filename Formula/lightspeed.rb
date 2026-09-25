class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.10"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.10/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "ef5f13d5bed567078b451259d75f3aabc6590101de46352559736acbf342a6c1"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.10/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "a82018f20087d6994d0d35b134876bfee4a283dc3a2d6e0af5e1e6a5d4a33217"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.10/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "5b460151689f7fc96ba9a243d6bda29d55cea6a177bff7b29fd96f0c9e11cdda"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.10/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "a7d390fc48d6cf7cbea40ebb43f56c92366f215029d16fbb0b3b293e2ed75b4d"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
