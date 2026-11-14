class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.10.3"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.3/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "f5c4ccf48a0b2794bf4199529e6dd9b85f6a5102e1649606c725d54e6cb6986f"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.3/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "41a0e5b40b3ca40881e744cd425bb4e7e72e894ff6bba657cbab6810681cbea9"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.3/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "994c6a14f7750f905a05bce5ff1fcb00acfa3c11a8ce5c1ee1dfbd0dcd3f0865"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.3/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "822c11e395404c693d4e4d119b62f46485faf23a7ebe9ac9a4c8775fc1110468"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
