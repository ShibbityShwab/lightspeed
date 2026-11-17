class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.11.0"
  # Custom noncommercial license (LightSpeed-NC-1.1), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.0/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "ad641669d4461c5afb04ada7550ed4fac8f9b87aebf833f5472755f424368818"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.0/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "aa3cacc4d1cb46bb34ec180c5614152ba0636e2715ec2d263684452ef30f4b5d"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.0/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "d77cb662c5b39610eb6b63b8866a223093053e378bba02ba4f27ebda2b3f3307"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.11.0/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "85183d651b3f2a1e326e184360e1d263c560bb678059d55a8e959e32721b43fe"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
