class Lightspeed < Formula
  desc "Zero-cost global network optimizer for multiplayer games"
  homepage "https://github.com/ShibbityShwab/lightspeed"
  version "1.10.2"
  # Custom noncommercial license (LightSpeed-NC-1.0), not an SPDX identifier.
  license :cannot_represent

  on_macos do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.2/lightspeed-client-x86_64-apple-darwin.tar.xz"
      sha256 "4a09f576555a936f170c85d37fb2bec0c0cba9a8801bb24771ddbffe4403a872"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.2/lightspeed-client-aarch64-apple-darwin.tar.xz"
      sha256 "04f1cb79e863c2c5f46269586be5928ffd01a59ccd585e5b297af6cf444f8432"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.2/lightspeed-client-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "b51d5e2fec41bd67be18424fce67d2e78a4a791ccd42ccdc6c867df777456efc"
    end
    on_arm do
      url "https://github.com/ShibbityShwab/lightspeed/releases/download/v1.10.2/lightspeed-client-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "0be61f699dd9ecf78b90a7af1731c2005c9fb6f25132ff403bfbdc2274db62b9"
    end
  end

  def install
    bin.install "lightspeed"
  end

  test do
    assert_match "lightspeed", shell_output("#{bin}/lightspeed --version")
  end
end
