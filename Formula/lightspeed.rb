class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.7"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.7/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "d4434c8ee2204375b621f68068cdcb7318b5d4c70d3321cf9c3f6765d61954d5"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.7/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "bd39e9dc2175a4ab6466e280f1562e5d122a5f5b68e796694fc4de0962cfe9d0"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.7/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "68fbadc064fa663d0c54ef3a066538456651c9e6bd9a5c85bbec71bc85a63dc5"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.7/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "ac74fba7b04cf12128c5fdcf8f5ffec9a1842f659e519d44999114e6eff8cefd"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
