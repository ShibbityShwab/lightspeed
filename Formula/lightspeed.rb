class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.12.0"
  # Custom noncommercial license (LightSpeed-NC-1.1), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.0/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "c2a4717fe705ea003bab1f808ce37efd00d640f89849eb96f31639f4ed891588"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.0/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "022c60d26988319e5492e1ee11ddff51a29016973e45d09f154fa56f4e52333f"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.0/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "0e95c6a17920f49c0a264c35b27973fc94f490a74c8595bde6696ae511bd8509"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.12.0/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "84a1b0565e88578ddb77cd420e3aff6e4f7fee279103210d6d32be4b53e8d55a"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
