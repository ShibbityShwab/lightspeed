class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.6.6"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.6/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "bcfc6d067bbf8bfcab7b50a5f3de2fd29832eb3d5fff6d4ddd0bca1e26c7ba70"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.6/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "3368f288dfd50a46e0d1aa2c637cc800d400b399fa9beaa88491d0f5a976dcf5"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.6/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "b4f02ff94503961e3c1a440910c05a2abe93d167fd81fa9587b3fffd2d5dd33d"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.6.6/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "4bd157a5808f1ae248824be20ab4e4863b1d585ccf2b2d630bcb3f5cc5c9cef6"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
